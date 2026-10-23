//! class・関数・method呼び出しとobject・scopeを扱う。

use super::*;
use crate::rowly_dsl::ast::{ClassDefinition, FunctionDefinition};

impl<'a> Runtime<'a> {
    // {
    //   責務: [instantiate_class: class instanceを生成し、親から子のfield初期化とInitを実行する]
    //   引数: [class_name: 生成するclass名, arguments: constructor引数式列]
    //   戻り値: [Result<Value, ExecutionError>: 新しいinstanceを指すObject値]
    //   エラー: [ExecutionError: class/継承解決、引数数、初期値式、constructorの失敗]
    // }
    pub(super) fn instantiate_class(
        &mut self,
        class_name: &str,
        arguments: &[Expression],
    ) -> Result<Value, ExecutionError> {
        let class = self.class_by_name(class_name)?;
        let lineage = self.class_lineage(&class.name)?;
        let constructor = self.find_method_in_hierarchy(&class.name, "Init")?;
        let expected = constructor
            .as_ref()
            .map(|(_, constructor)| constructor.parameters.len())
            .unwrap_or(0);
        if expected != arguments.len() {
            return Err(ExecutionError::ConstructorArgumentCount {
                class_name: class.name.clone(),
                expected,
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;

        let mut fields = HashMap::new();
        for current in &lineage {
            for field in &current.fields {
                let value = self.evaluate_expression(&field.default)?;
                fields.insert(normalize_identifier(&field.name), value);
            }
        }
        let object_id = self.objects.len();
        self.objects.push(ObjectInstance {
            class_name: class.name.clone(),
            fields,
        });
        self.events.push(ExecutionEvent::ObjectCreated {
            class_name: class.name.clone(),
        });

        if let Some((defining_class, constructor)) = constructor {
            let _ =
                self.invoke_method_with_values(object_id, defining_class, constructor, values)?;
        }

        Ok(Value::Object(object_id))
    }

    // {
    //   責務: [call_function: top-level functionをlocal scopeで呼び出す]
    //   処理: [引数を評価し、呼び出しeventを記録してscopeと深度を復元する]
    //   引数: [name: 関数名, arguments: 引数式列]
    //   戻り値: [Result<Option<Value>, ExecutionError>: Return値。明示ReturnがなければNone]
    //   エラー: [ExecutionError: 関数解決、引数数、深度上限、またはbody実行の失敗]
    // }
    pub(super) fn call_function(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<Value>, ExecutionError> {
        self.ensure_call_depth()?;
        let program = self.program;
        let function = program
            .functions
            .iter()
            .find(|function| function.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| ExecutionError::UnknownFunction(name.to_owned()))?;
        if function.parameters.len() != arguments.len() {
            return Err(ExecutionError::ArgumentCount {
                name: function.name.clone(),
                expected: function.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        let argument_descriptions = values
            .iter()
            .map(|value| self.describe_value(value))
            .collect();
        let scope = function
            .parameters
            .iter()
            .zip(values)
            .map(|(parameter, value)| (normalize_identifier(parameter), Binding::variable(value)))
            .collect();
        self.scopes.push(scope);
        self.call_depth += 1;
        let execution = self.execute_statements(&function.body);
        self.call_depth -= 1;
        self.scopes.pop();
        let return_value = match execution? {
            Flow::Continue => None,
            Flow::Return(value) => value,
        };
        self.events.push(ExecutionEvent::FunctionCalled {
            name: function.name.clone(),
            arguments: argument_descriptions,
            return_value: return_value
                .as_ref()
                .map(|value| self.describe_value(value)),
        });
        Ok(return_value)
    }

    // {
    //   責務: [call_method: instanceのclass階層からmethodを解決して呼び出す]
    //   引数: [object_id: 呼び出し対象ID, name: method名, arguments: 引数式列]
    //   戻り値: [Result<Option<Value>, ExecutionError>: Return値またはNone]
    //   エラー: [ExecutionError: instance、method、引数数、または実行失敗]
    // }
    pub(super) fn call_method(
        &mut self,
        object_id: ObjectId,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<Value>, ExecutionError> {
        let class_name = self.object(object_id)?.class_name.clone();
        let (defining_class, method) = self
            .find_method_in_hierarchy(&class_name, name)?
            .ok_or_else(|| ExecutionError::UnknownMethod {
                class_name: class_name.clone(),
                method: name.to_owned(),
            })?;
        if method.parameters.len() != arguments.len() {
            return Err(ExecutionError::MethodArgumentCount {
                class_name,
                name: method.name.clone(),
                expected: method.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        self.invoke_method_with_values(object_id, defining_class, method, values)
    }

    // {
    //   責務: [call_super_method: 現在のmethod定義元より上位からSuper methodを解決する]
    //   引数: [name: 親側method名, arguments: 引数式列]
    //   戻り値: [Result<Option<Value>, ExecutionError>: 親methodのReturn値]
    //   エラー: [ExecutionError: method外、親なし、method不明、引数数またはbodyの失敗]
    // }
    pub(super) fn call_super_method(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<Value>, ExecutionError> {
        let current_class = self
            .method_context
            .last()
            .cloned()
            .ok_or(ExecutionError::SuperOutsideMethod)?;
        let parent = self
            .class_by_name(&current_class)?
            .parent
            .as_deref()
            .ok_or_else(|| ExecutionError::NoSuperClass {
                class_name: current_class.clone(),
            })?;
        let object_id = self.resolve_object("Self")?;
        let (defining_class, method) =
            self.find_method_in_hierarchy(parent, name)?
                .ok_or_else(|| ExecutionError::UnknownSuperMethod {
                    class_name: current_class,
                    method: name.to_owned(),
                })?;
        if method.parameters.len() != arguments.len() {
            return Err(ExecutionError::MethodArgumentCount {
                class_name: defining_class.to_owned(),
                name: method.name.clone(),
                expected: method.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        self.invoke_method_with_values(object_id, defining_class, method, values)
    }

    // {
    //   責務: [invoke_method_with_values: 評価済み引数でmethod本体を実行する]
    //   処理: [SelfをCONSTで束縛し、scope / method context / call depthを復元する]
    //   引数: [object_id: 対象instance, defining_class: method定義class, method: method AST, values: 引数値]
    //   戻り値: [Result<Option<Value>, ExecutionError>: methodのReturn値またはNone]
    //   エラー: [ExecutionError: 呼び出し深度またはmethod bodyの失敗]
    // }
    pub(super) fn invoke_method_with_values(
        &mut self,
        object_id: ObjectId,
        defining_class: &str,
        method: &FunctionDefinition,
        values: Vec<Value>,
    ) -> Result<Option<Value>, ExecutionError> {
        self.ensure_call_depth()?;
        let argument_descriptions = values
            .iter()
            .map(|value| self.describe_value(value))
            .collect();
        let mut scope: HashMap<String, Binding> = method
            .parameters
            .iter()
            .zip(values)
            .map(|(parameter, value)| (normalize_identifier(parameter), Binding::variable(value)))
            .collect();
        scope.insert(
            "self".to_owned(),
            Binding {
                value: Value::Object(object_id),
                kind: DeclarationKind::Const,
            },
        );
        self.scopes.push(scope);
        self.method_context.push(defining_class.to_owned());
        self.call_depth += 1;
        let execution = self.execute_statements(&method.body);
        self.call_depth -= 1;
        self.method_context.pop();
        self.scopes.pop();
        let return_value = match execution? {
            Flow::Continue => None,
            Flow::Return(value) => value,
        };
        self.events.push(ExecutionEvent::MethodCalled {
            class_name: defining_class.to_owned(),
            name: method.name.clone(),
            arguments: argument_descriptions,
            return_value: return_value
                .as_ref()
                .map(|value| self.describe_value(value)),
        });
        Ok(return_value)
    }

    // {
    //   責務: [class_by_name: 大文字小文字を無視してProgramからclassを検索する]
    //   引数: [name: 検索するclass名]
    //   戻り値: [Result<&ClassDefinition, ExecutionError>: class定義参照]
    //   エラー: [ExecutionError: classが存在しない]
    // }
    pub(super) fn class_by_name(&self, name: &str) -> Result<&'a ClassDefinition, ExecutionError> {
        self.program
            .classes
            .iter()
            .find(|class| class.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| ExecutionError::UnknownClass(name.to_owned()))
    }

    // {
    //   責務: [class_lineage: class自身からroot parentまでの継承列を構築する]
    //   引数: [class_name: 起点class名]
    //   戻り値: [Result<Vec<&ClassDefinition>, ExecutionError>: rootからchild順のclass列]
    //   エラー: [ExecutionError: class/parent不明または継承cycle]
    // }
    pub(super) fn class_lineage(
        &self,
        class_name: &str,
    ) -> Result<Vec<&'a ClassDefinition>, ExecutionError> {
        let mut lineage = Vec::new();
        let mut seen = Vec::new();
        let mut current = self.class_by_name(class_name)?;

        loop {
            let key = normalize_identifier(&current.name);
            if seen.iter().any(|name| name == &key) {
                return Err(ExecutionError::InheritanceCycle(current.name.clone()));
            }
            seen.push(key);
            lineage.push(current);

            let Some(parent_name) = current.parent.as_deref() else {
                break;
            };
            current = self
                .program
                .classes
                .iter()
                .find(|class| class.name.eq_ignore_ascii_case(parent_name))
                .ok_or_else(|| ExecutionError::UnknownParentClass {
                    class_name: current.name.clone(),
                    parent: parent_name.to_owned(),
                })?;
        }

        lineage.reverse();
        Ok(lineage)
    }

    // {
    //   責務: [find_method_in_hierarchy: 最も派生したclassからmethodを探す]
    //   引数: [class_name: 検索開始class名, method_name: method名]
    //   戻り値: [Result<Option<(&str, &FunctionDefinition)>, ExecutionError>: 定義元classとmethod]
    //   エラー: [ExecutionError: 継承列を解決できない]
    // }
    pub(super) fn find_method_in_hierarchy(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Result<Option<(&'a str, &'a FunctionDefinition)>, ExecutionError> {
        let lineage = self.class_lineage(class_name)?;
        Ok(lineage.iter().rev().find_map(|class| {
            class
                .methods
                .iter()
                .find(|method| method.name.eq_ignore_ascii_case(method_name))
                .map(|method| (class.name.as_str(), method))
        }))
    }

    // {
    //   責務: [evaluate_arguments: 引数式列を左から順番に評価する]
    //   引数: [arguments: 引数式列]
    //   戻り値: [Result<Vec<Value>, ExecutionError>: 評価済み引数値列]
    //   エラー: [ExecutionError: いずれかの引数評価が失敗]
    // }
    pub(super) fn evaluate_arguments(
        &mut self,
        arguments: &[Expression],
    ) -> Result<Vec<Value>, ExecutionError> {
        arguments
            .iter()
            .map(|argument| self.evaluate_expression(argument))
            .collect()
    }

    // {
    //   責務: [ensure_call_depth: 次のfunction/method呼び出しが深度上限内か検査する]
    //   引数: [self: 現在のcall depthを持つruntime]
    //   戻り値: [Result<(), ExecutionError>: 上限未満ならOk(())]
    //   エラー: [ExecutionError: MAX_CALL_DEPTHに達している]
    // }
    pub(super) fn ensure_call_depth(&self) -> Result<(), ExecutionError> {
        if self.call_depth >= MAX_CALL_DEPTH {
            Err(ExecutionError::CallDepthExceeded {
                limit: MAX_CALL_DEPTH,
            })
        } else {
            Ok(())
        }
    }

    // {
    //   責務: [resolve_object: variable名をobject instance IDへ解決する]
    //   引数: [variable: objectを保持するvariable名]
    //   戻り値: [Result<ObjectId, ExecutionError>: object配列の識別子]
    //   エラー: [ExecutionError: variableが不明またはobjectでない]
    // }
    pub(super) fn resolve_object(&self, variable: &str) -> Result<ObjectId, ExecutionError> {
        match self.lookup_variable(variable) {
            Some(Value::Object(object_id)) => Ok(*object_id),
            Some(_) => Err(ExecutionError::ExpectedObject(variable.to_owned())),
            None => Err(ExecutionError::UnknownVariable(variable.to_owned())),
        }
    }

    // {
    //   責務: [get_field: objectから大小文字を無視してfield値を取得する]
    //   引数: [object_id: 対象object ID, field: field名]
    //   戻り値: [Result<Value, ExecutionError>: cloneしたfield値]
    //   エラー: [ExecutionError: object IDまたはfield名が不正]
    // }
    pub(super) fn get_field(
        &self,
        object_id: ObjectId,
        field: &str,
    ) -> Result<Value, ExecutionError> {
        let object = self.object(object_id)?;
        object
            .fields
            .get(&normalize_identifier(field))
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownField {
                class_name: object.class_name.clone(),
                field: field.to_owned(),
            })
    }

    // {
    //   責務: [set_field: 既存fieldの値を更新する]
    //   引数: [object_id: 対象object ID, field: field名, value: 新しい値]
    //   戻り値: [Result<(), ExecutionError>: 更新成功時Ok(())]
    //   エラー: [ExecutionError: object IDまたはfield名が不正]
    // }
    pub(super) fn set_field(
        &mut self,
        object_id: ObjectId,
        field: &str,
        value: Value,
    ) -> Result<(), ExecutionError> {
        let key = normalize_identifier(field);
        let object = self.object_mut(object_id)?;
        if !object.fields.contains_key(&key) {
            return Err(ExecutionError::UnknownField {
                class_name: object.class_name.clone(),
                field: field.to_owned(),
            });
        }
        object.fields.insert(key, value);
        Ok(())
    }

    // {
    //   責務: [object: IDからimmutableなinstance参照を取得する]
    //   引数: [object_id: objects配列の識別子]
    //   戻り値: [Result<&ObjectInstance, ExecutionError>: instance参照]
    //   エラー: [ExecutionError: IDがobjects配列の範囲外]
    // }
    pub(super) fn object(&self, object_id: ObjectId) -> Result<&ObjectInstance, ExecutionError> {
        self.objects
            .get(object_id)
            .ok_or_else(|| ExecutionError::ExpectedObject(format!("object#{object_id}")))
    }

    // {
    //   責務: [object_mut: IDからmutableなinstance参照を取得する]
    //   引数: [object_id: objects配列の識別子]
    //   戻り値: [Result<&mut ObjectInstance, ExecutionError>: instanceのmutable参照]
    //   エラー: [ExecutionError: IDがobjects配列の範囲外]
    // }
    pub(super) fn object_mut(
        &mut self,
        object_id: ObjectId,
    ) -> Result<&mut ObjectInstance, ExecutionError> {
        self.objects
            .get_mut(object_id)
            .ok_or_else(|| ExecutionError::ExpectedObject(format!("object#{object_id}")))
    }

    // {
    //   責務: [lookup_variable: 現在scopeから外側へbindingを検索する]
    //   引数: [name: 検索するvariable名]
    //   戻り値: [Option<&Value>: 最も近いscopeの値]
    // }
    pub(super) fn lookup_variable(&self, name: &str) -> Option<&Value> {
        let key = normalize_identifier(name);
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&key).map(|binding| &binding.value))
    }

    // {
    //   責務: [current_scope_mut: 宣言・代入対象の現在scopeを取得する]
    //   引数: [self: scope stackを持つruntime]
    //   戻り値: [&mut HashMap<String, Binding>: 最内側scope]
    //   前提: [Runtimeはglobal scopeを常に最低1つ保持する]
    // }
    pub(super) fn current_scope_mut(&mut self) -> &mut HashMap<String, Binding> {
        self.scopes
            .last_mut()
            .expect("runtime always has at least the global scope")
    }
}
