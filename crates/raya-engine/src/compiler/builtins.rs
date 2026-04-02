use crate::semantics::{HostHandleOpKind, IteratorOpKind, JsOpKind, MetaobjectOpKind};

pub type BuiltinOpId = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinOp {
    Metaobject(MetaobjectOpKind),
    Iterator(IteratorOpKind),
    HostHandle(HostHandleOpKind),
    Js(JsOpKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinSurfaceKind {
    Constructor,
    InstanceMethod,
    StaticMethod,
    PropertyGet,
    PropertySet,
    NamespaceCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandleKind {
    Mutex,
    Channel,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinReceiverModel {
    None,
    Value,
    Object,
    Handle(HandleKind),
    TaskHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinExecutionKind {
    PureOpcode,
    RuntimeBuiltin,
    ResumableRuntimeBuiltin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuiltinDescriptor {
    pub op: BuiltinOp,
    pub name: &'static str,
    pub surface: BuiltinSurfaceKind,
    pub receiver: BuiltinReceiverModel,
    pub execution: BuiltinExecutionKind,
}

const METAOBJECT_COUNT: BuiltinOpId = 13;
const ITERATOR_COUNT: BuiltinOpId = 12;
const HOST_HANDLE_COUNT: BuiltinOpId = 17;
const JS_COUNT: BuiltinOpId = 30;

const METAOBJECT_BASE: BuiltinOpId = 0;
const ITERATOR_BASE: BuiltinOpId = METAOBJECT_BASE + METAOBJECT_COUNT;
const HOST_HANDLE_BASE: BuiltinOpId = ITERATOR_BASE + ITERATOR_COUNT;
const JS_BASE: BuiltinOpId = HOST_HANDLE_BASE + HOST_HANDLE_COUNT;
const BUILTIN_COUNT: BuiltinOpId = JS_BASE + JS_COUNT;

impl From<MetaobjectOpKind> for BuiltinOp {
    fn from(value: MetaobjectOpKind) -> Self {
        Self::Metaobject(value)
    }
}

impl From<IteratorOpKind> for BuiltinOp {
    fn from(value: IteratorOpKind) -> Self {
        Self::Iterator(value)
    }
}

impl From<HostHandleOpKind> for BuiltinOp {
    fn from(value: HostHandleOpKind) -> Self {
        Self::HostHandle(value)
    }
}

impl From<JsOpKind> for BuiltinOp {
    fn from(value: JsOpKind) -> Self {
        Self::Js(value)
    }
}

pub fn builtin_descriptor(op: BuiltinOp) -> BuiltinDescriptor {
    match op {
        BuiltinOp::Metaobject(kind) => BuiltinDescriptor {
            op,
            name: match kind {
                MetaobjectOpKind::DefineProperty => "metaobject.defineProperty",
                MetaobjectOpKind::GetOwnPropertyDescriptor => "metaobject.getOwnPropertyDescriptor",
                MetaobjectOpKind::DefineProperties => "metaobject.defineProperties",
                MetaobjectOpKind::DeleteProperty => "metaobject.deleteProperty",
                MetaobjectOpKind::GetPrototypeOf => "metaobject.getPrototypeOf",
                MetaobjectOpKind::SetPrototypeOf => "metaobject.setPrototypeOf",
                MetaobjectOpKind::PreventExtensions => "metaobject.preventExtensions",
                MetaobjectOpKind::IsExtensible => "metaobject.isExtensible",
                MetaobjectOpKind::ReflectGet => "metaobject.reflectGet",
                MetaobjectOpKind::ReflectSet => "metaobject.reflectSet",
                MetaobjectOpKind::ReflectHas => "metaobject.reflectHas",
                MetaobjectOpKind::ReflectOwnKeys => "metaobject.reflectOwnKeys",
                MetaobjectOpKind::ReflectConstruct => "metaobject.reflectConstruct",
            },
            surface: BuiltinSurfaceKind::NamespaceCall,
            receiver: BuiltinReceiverModel::Object,
            execution: BuiltinExecutionKind::RuntimeBuiltin,
        },
        BuiltinOp::Iterator(kind) => BuiltinDescriptor {
            op,
            name: match kind {
                IteratorOpKind::GetIterator => "iterator.get",
                IteratorOpKind::GetAsyncIterator => "iterator.getAsync",
                IteratorOpKind::Step => "iterator.step",
                IteratorOpKind::Done => "iterator.done",
                IteratorOpKind::Value => "iterator.value",
                IteratorOpKind::ResumeNext => "iterator.resumeNext",
                IteratorOpKind::ResumeReturn => "iterator.resumeReturn",
                IteratorOpKind::ResumeThrow => "iterator.resumeThrow",
                IteratorOpKind::Close => "iterator.close",
                IteratorOpKind::CloseOnThrow => "iterator.closeOnThrow",
                IteratorOpKind::CloseCompletion => "iterator.closeCompletion",
                IteratorOpKind::AppendToArray => "iterator.appendToArray",
            },
            surface: BuiltinSurfaceKind::NamespaceCall,
            receiver: BuiltinReceiverModel::Value,
            execution: BuiltinExecutionKind::RuntimeBuiltin,
        },
        BuiltinOp::HostHandle(kind) => BuiltinDescriptor {
            op,
            name: match kind {
                HostHandleOpKind::ChannelConstructor => "channel.constructor",
                HostHandleOpKind::ChannelSend => "channel.send",
                HostHandleOpKind::ChannelReceive => "channel.receive",
                HostHandleOpKind::ChannelTrySend => "channel.trySend",
                HostHandleOpKind::ChannelTryReceive => "channel.tryReceive",
                HostHandleOpKind::ChannelClose => "channel.close",
                HostHandleOpKind::ChannelIsClosed => "channel.isClosed",
                HostHandleOpKind::ChannelLength => "channel.length",
                HostHandleOpKind::ChannelCapacity => "channel.capacity",
                HostHandleOpKind::MutexConstructor => "mutex.constructor",
                HostHandleOpKind::MutexLock => "mutex.lock",
                HostHandleOpKind::MutexUnlock => "mutex.unlock",
                HostHandleOpKind::MutexTryLock => "mutex.tryLock",
                HostHandleOpKind::MutexIsLocked => "mutex.isLocked",
                HostHandleOpKind::TaskCancel => "promise.cancel",
                HostHandleOpKind::TaskIsDone => "promise.isDone",
                HostHandleOpKind::TaskIsCancelled => "promise.isCancelled",
            },
            surface: match kind {
                HostHandleOpKind::ChannelConstructor | HostHandleOpKind::MutexConstructor => {
                    BuiltinSurfaceKind::Constructor
                }
                _ => BuiltinSurfaceKind::InstanceMethod,
            },
            receiver: match kind {
                HostHandleOpKind::ChannelConstructor | HostHandleOpKind::MutexConstructor => {
                    BuiltinReceiverModel::None
                }
                HostHandleOpKind::ChannelSend
                | HostHandleOpKind::ChannelReceive
                | HostHandleOpKind::ChannelTrySend
                | HostHandleOpKind::ChannelTryReceive
                | HostHandleOpKind::ChannelClose
                | HostHandleOpKind::ChannelIsClosed
                | HostHandleOpKind::ChannelLength
                | HostHandleOpKind::ChannelCapacity => BuiltinReceiverModel::Handle(HandleKind::Channel),
                HostHandleOpKind::MutexLock
                | HostHandleOpKind::MutexUnlock
                | HostHandleOpKind::MutexTryLock
                | HostHandleOpKind::MutexIsLocked => BuiltinReceiverModel::Handle(HandleKind::Mutex),
                HostHandleOpKind::TaskCancel
                | HostHandleOpKind::TaskIsDone
                | HostHandleOpKind::TaskIsCancelled => BuiltinReceiverModel::TaskHandle,
            },
            execution: BuiltinExecutionKind::RuntimeBuiltin,
        },
        BuiltinOp::Js(kind) => BuiltinDescriptor {
            op,
            name: match kind {
                JsOpKind::GetNamed => "js.getNamed",
                JsOpKind::GetKeyed => "js.getKeyed",
                JsOpKind::SetNamed { strict: false } => "js.setNamed",
                JsOpKind::SetNamed { strict: true } => "js.setNamedStrict",
                JsOpKind::SetKeyed { strict: false } => "js.setKeyed",
                JsOpKind::SetKeyed { strict: true } => "js.setKeyedStrict",
                JsOpKind::BindMethod => "js.bindMethod",
                JsOpKind::ResolveIdentifier { non_throwing: false } => "js.resolveIdentifier",
                JsOpKind::ResolveIdentifier { non_throwing: true } => {
                    "js.resolveIdentifierNonThrowing"
                }
                JsOpKind::HasIdentifier => "js.hasIdentifier",
                JsOpKind::AssignIdentifier { strict: false } => "js.assignIdentifier",
                JsOpKind::AssignIdentifier { strict: true } => "js.assignIdentifierStrict",
                JsOpKind::DeleteIdentifier => "js.deleteIdentifier",
                JsOpKind::DeclareVar => "js.declareVar",
                JsOpKind::DeclareFunction => "js.declareFunction",
                JsOpKind::DeclareLexical => "js.declareLexical",
                JsOpKind::CallValue => "js.callValue",
                JsOpKind::CallMemberNamed => "js.callMemberNamed",
                JsOpKind::CallMemberKeyed => "js.callMemberKeyed",
                JsOpKind::ConstructValue => "js.constructValue",
                JsOpKind::EnterActivationEnv => "js.enterActivationEnv",
                JsOpKind::LeaveActivationEnv => "js.leaveActivationEnv",
                JsOpKind::PushWithEnv => "js.pushWithEnv",
                JsOpKind::PopWithEnv => "js.popWithEnv",
                JsOpKind::PushDeclarativeEnv => "js.pushDeclarativeEnv",
                JsOpKind::PopDeclarativeEnv => "js.popDeclarativeEnv",
                JsOpKind::ReplaceDeclarativeEnv => "js.replaceDeclarativeEnv",
                JsOpKind::DirectEval => "js.directEval",
                JsOpKind::EvalGetCompletion => "js.evalGetCompletion",
                JsOpKind::EvalSetCompletion => "js.evalSetCompletion",
            },
            surface: BuiltinSurfaceKind::NamespaceCall,
            receiver: match kind {
                JsOpKind::GetNamed
                | JsOpKind::GetKeyed
                | JsOpKind::SetNamed { .. }
                | JsOpKind::SetKeyed { .. }
                | JsOpKind::BindMethod
                | JsOpKind::CallMemberNamed
                | JsOpKind::CallMemberKeyed => BuiltinReceiverModel::Object,
                JsOpKind::CallValue | JsOpKind::ConstructValue => BuiltinReceiverModel::Value,
                _ => BuiltinReceiverModel::None,
            },
            execution: BuiltinExecutionKind::RuntimeBuiltin,
        },
    }
}

pub fn encode_builtin_op_id(op: BuiltinOp) -> BuiltinOpId {
    match op {
        BuiltinOp::Metaobject(kind) => METAOBJECT_BASE
            + match kind {
                MetaobjectOpKind::DefineProperty => 0,
                MetaobjectOpKind::GetOwnPropertyDescriptor => 1,
                MetaobjectOpKind::DefineProperties => 2,
                MetaobjectOpKind::DeleteProperty => 3,
                MetaobjectOpKind::GetPrototypeOf => 4,
                MetaobjectOpKind::SetPrototypeOf => 5,
                MetaobjectOpKind::PreventExtensions => 6,
                MetaobjectOpKind::IsExtensible => 7,
                MetaobjectOpKind::ReflectGet => 8,
                MetaobjectOpKind::ReflectSet => 9,
                MetaobjectOpKind::ReflectHas => 10,
                MetaobjectOpKind::ReflectOwnKeys => 11,
                MetaobjectOpKind::ReflectConstruct => 12,
            },
        BuiltinOp::Iterator(kind) => ITERATOR_BASE
            + match kind {
                IteratorOpKind::GetIterator => 0,
                IteratorOpKind::GetAsyncIterator => 1,
                IteratorOpKind::Step => 2,
                IteratorOpKind::Done => 3,
                IteratorOpKind::Value => 4,
                IteratorOpKind::ResumeNext => 5,
                IteratorOpKind::ResumeReturn => 6,
                IteratorOpKind::ResumeThrow => 7,
                IteratorOpKind::Close => 8,
                IteratorOpKind::CloseOnThrow => 9,
                IteratorOpKind::CloseCompletion => 10,
                IteratorOpKind::AppendToArray => 11,
            },
        BuiltinOp::HostHandle(kind) => HOST_HANDLE_BASE
            + match kind {
                HostHandleOpKind::ChannelConstructor => 0,
                HostHandleOpKind::ChannelSend => 1,
                HostHandleOpKind::ChannelReceive => 2,
                HostHandleOpKind::ChannelTrySend => 3,
                HostHandleOpKind::ChannelTryReceive => 4,
                HostHandleOpKind::ChannelClose => 5,
                HostHandleOpKind::ChannelIsClosed => 6,
                HostHandleOpKind::ChannelLength => 7,
                HostHandleOpKind::ChannelCapacity => 8,
                HostHandleOpKind::MutexConstructor => 9,
                HostHandleOpKind::MutexLock => 10,
                HostHandleOpKind::MutexUnlock => 11,
                HostHandleOpKind::MutexTryLock => 12,
                HostHandleOpKind::MutexIsLocked => 13,
                HostHandleOpKind::TaskCancel => 14,
                HostHandleOpKind::TaskIsDone => 15,
                HostHandleOpKind::TaskIsCancelled => 16,
            },
        BuiltinOp::Js(kind) => JS_BASE
            + match kind {
                JsOpKind::GetNamed => 0,
                JsOpKind::GetKeyed => 1,
                JsOpKind::SetNamed { strict: false } => 2,
                JsOpKind::SetNamed { strict: true } => 3,
                JsOpKind::SetKeyed { strict: false } => 4,
                JsOpKind::SetKeyed { strict: true } => 5,
                JsOpKind::BindMethod => 6,
                JsOpKind::ResolveIdentifier { non_throwing: false } => 7,
                JsOpKind::ResolveIdentifier { non_throwing: true } => 8,
                JsOpKind::AssignIdentifier { strict: false } => 9,
                JsOpKind::AssignIdentifier { strict: true } => 10,
                JsOpKind::CallValue => 11,
                JsOpKind::CallMemberNamed => 12,
                JsOpKind::CallMemberKeyed => 13,
                JsOpKind::ConstructValue => 14,
                JsOpKind::PushWithEnv => 15,
                JsOpKind::PopWithEnv => 16,
                JsOpKind::PushDeclarativeEnv => 17,
                JsOpKind::PopDeclarativeEnv => 18,
                JsOpKind::ReplaceDeclarativeEnv => 19,
                JsOpKind::DirectEval => 20,
                JsOpKind::EvalGetCompletion => 21,
                JsOpKind::EvalSetCompletion => 22,
                JsOpKind::HasIdentifier => 23,
                JsOpKind::DeleteIdentifier => 24,
                JsOpKind::DeclareVar => 25,
                JsOpKind::DeclareFunction => 26,
                JsOpKind::DeclareLexical => 27,
                JsOpKind::EnterActivationEnv => 28,
                JsOpKind::LeaveActivationEnv => 29,
            },
    }
}

pub fn decode_builtin_op_id(id: BuiltinOpId) -> Option<BuiltinOp> {
    if id >= BUILTIN_COUNT {
        return None;
    }
    if id >= JS_BASE {
        return Some(BuiltinOp::Js(match id - JS_BASE {
            0 => JsOpKind::GetNamed,
            1 => JsOpKind::GetKeyed,
            2 => JsOpKind::SetNamed { strict: false },
            3 => JsOpKind::SetNamed { strict: true },
            4 => JsOpKind::SetKeyed { strict: false },
            5 => JsOpKind::SetKeyed { strict: true },
            6 => JsOpKind::BindMethod,
            7 => JsOpKind::ResolveIdentifier { non_throwing: false },
            8 => JsOpKind::ResolveIdentifier { non_throwing: true },
            9 => JsOpKind::AssignIdentifier { strict: false },
            10 => JsOpKind::AssignIdentifier { strict: true },
            11 => JsOpKind::CallValue,
            12 => JsOpKind::CallMemberNamed,
            13 => JsOpKind::CallMemberKeyed,
            14 => JsOpKind::ConstructValue,
            15 => JsOpKind::PushWithEnv,
            16 => JsOpKind::PopWithEnv,
            17 => JsOpKind::PushDeclarativeEnv,
            18 => JsOpKind::PopDeclarativeEnv,
            19 => JsOpKind::ReplaceDeclarativeEnv,
            20 => JsOpKind::DirectEval,
            21 => JsOpKind::EvalGetCompletion,
            22 => JsOpKind::EvalSetCompletion,
            23 => JsOpKind::HasIdentifier,
            24 => JsOpKind::DeleteIdentifier,
            25 => JsOpKind::DeclareVar,
            26 => JsOpKind::DeclareFunction,
            27 => JsOpKind::DeclareLexical,
            28 => JsOpKind::EnterActivationEnv,
            29 => JsOpKind::LeaveActivationEnv,
            _ => unreachable!(),
        }));
    }
    if id >= HOST_HANDLE_BASE {
        return Some(BuiltinOp::HostHandle(match id - HOST_HANDLE_BASE {
            0 => HostHandleOpKind::ChannelConstructor,
            1 => HostHandleOpKind::ChannelSend,
            2 => HostHandleOpKind::ChannelReceive,
            3 => HostHandleOpKind::ChannelTrySend,
            4 => HostHandleOpKind::ChannelTryReceive,
            5 => HostHandleOpKind::ChannelClose,
            6 => HostHandleOpKind::ChannelIsClosed,
            7 => HostHandleOpKind::ChannelLength,
            8 => HostHandleOpKind::ChannelCapacity,
            9 => HostHandleOpKind::MutexConstructor,
            10 => HostHandleOpKind::MutexLock,
            11 => HostHandleOpKind::MutexUnlock,
            12 => HostHandleOpKind::MutexTryLock,
            13 => HostHandleOpKind::MutexIsLocked,
            14 => HostHandleOpKind::TaskCancel,
            15 => HostHandleOpKind::TaskIsDone,
            16 => HostHandleOpKind::TaskIsCancelled,
            _ => unreachable!(),
        }));
    }
    if id >= ITERATOR_BASE {
        return Some(BuiltinOp::Iterator(match id - ITERATOR_BASE {
            0 => IteratorOpKind::GetIterator,
            1 => IteratorOpKind::GetAsyncIterator,
            2 => IteratorOpKind::Step,
            3 => IteratorOpKind::Done,
            4 => IteratorOpKind::Value,
            5 => IteratorOpKind::ResumeNext,
            6 => IteratorOpKind::ResumeReturn,
            7 => IteratorOpKind::ResumeThrow,
            8 => IteratorOpKind::Close,
            9 => IteratorOpKind::CloseOnThrow,
            10 => IteratorOpKind::CloseCompletion,
            11 => IteratorOpKind::AppendToArray,
            _ => unreachable!(),
        }));
    }
    Some(BuiltinOp::Metaobject(match id - METAOBJECT_BASE {
        0 => MetaobjectOpKind::DefineProperty,
        1 => MetaobjectOpKind::GetOwnPropertyDescriptor,
        2 => MetaobjectOpKind::DefineProperties,
        3 => MetaobjectOpKind::DeleteProperty,
        4 => MetaobjectOpKind::GetPrototypeOf,
        5 => MetaobjectOpKind::SetPrototypeOf,
        6 => MetaobjectOpKind::PreventExtensions,
        7 => MetaobjectOpKind::IsExtensible,
        8 => MetaobjectOpKind::ReflectGet,
        9 => MetaobjectOpKind::ReflectSet,
        10 => MetaobjectOpKind::ReflectHas,
        11 => MetaobjectOpKind::ReflectOwnKeys,
        12 => MetaobjectOpKind::ReflectConstruct,
        _ => unreachable!(),
    }))
}

pub fn builtin_op_from_native_id(native_id: u16) -> Option<BuiltinOp> {
    Some(match native_id {
        crate::compiler::native_id::OBJECT_DEFINE_PROPERTY => {
            BuiltinOp::Metaobject(MetaobjectOpKind::DefineProperty)
        }
        crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR => {
            BuiltinOp::Metaobject(MetaobjectOpKind::GetOwnPropertyDescriptor)
        }
        crate::compiler::native_id::OBJECT_DEFINE_PROPERTIES => {
            BuiltinOp::Metaobject(MetaobjectOpKind::DefineProperties)
        }
        crate::compiler::native_id::OBJECT_DELETE_PROPERTY => {
            BuiltinOp::Metaobject(MetaobjectOpKind::DeleteProperty)
        }
        crate::compiler::native_id::OBJECT_GET_PROTOTYPE_OF => {
            BuiltinOp::Metaobject(MetaobjectOpKind::GetPrototypeOf)
        }
        crate::compiler::native_id::OBJECT_SET_PROTOTYPE_OF => {
            BuiltinOp::Metaobject(MetaobjectOpKind::SetPrototypeOf)
        }
        crate::compiler::native_id::OBJECT_PREVENT_EXTENSIONS => {
            BuiltinOp::Metaobject(MetaobjectOpKind::PreventExtensions)
        }
        crate::compiler::native_id::OBJECT_IS_EXTENSIBLE => {
            BuiltinOp::Metaobject(MetaobjectOpKind::IsExtensible)
        }
        crate::compiler::native_id::REFLECT_GET => BuiltinOp::Metaobject(MetaobjectOpKind::ReflectGet),
        crate::compiler::native_id::REFLECT_SET => BuiltinOp::Metaobject(MetaobjectOpKind::ReflectSet),
        crate::compiler::native_id::REFLECT_HAS => BuiltinOp::Metaobject(MetaobjectOpKind::ReflectHas),
        crate::compiler::native_id::REFLECT_OWN_KEYS => {
            BuiltinOp::Metaobject(MetaobjectOpKind::ReflectOwnKeys)
        }
        crate::compiler::native_id::REFLECT_CONSTRUCT => {
            BuiltinOp::Metaobject(MetaobjectOpKind::ReflectConstruct)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_GET => {
            BuiltinOp::Iterator(IteratorOpKind::GetIterator)
        }
        crate::compiler::native_id::OBJECT_ASYNC_ITERATOR_GET => {
            BuiltinOp::Iterator(IteratorOpKind::GetAsyncIterator)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_STEP => {
            BuiltinOp::Iterator(IteratorOpKind::Step)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_DONE => {
            BuiltinOp::Iterator(IteratorOpKind::Done)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_VALUE => {
            BuiltinOp::Iterator(IteratorOpKind::Value)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_RESUME_NEXT => {
            BuiltinOp::Iterator(IteratorOpKind::ResumeNext)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_RESUME_RETURN => {
            BuiltinOp::Iterator(IteratorOpKind::ResumeReturn)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_RESUME_THROW => {
            BuiltinOp::Iterator(IteratorOpKind::ResumeThrow)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_CLOSE => {
            BuiltinOp::Iterator(IteratorOpKind::Close)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_ON_THROW => {
            BuiltinOp::Iterator(IteratorOpKind::CloseOnThrow)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_COMPLETION => {
            BuiltinOp::Iterator(IteratorOpKind::CloseCompletion)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_APPEND_TO_ARRAY => {
            BuiltinOp::Iterator(IteratorOpKind::AppendToArray)
        }
        crate::compiler::native_id::CHANNEL_NEW => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelConstructor)
        }
        crate::compiler::native_id::CHANNEL_SEND => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelSend)
        }
        crate::compiler::native_id::CHANNEL_RECEIVE => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelReceive)
        }
        crate::compiler::native_id::CHANNEL_TRY_SEND => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelTrySend)
        }
        crate::compiler::native_id::CHANNEL_TRY_RECEIVE => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelTryReceive)
        }
        crate::compiler::native_id::CHANNEL_CLOSE => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelClose)
        }
        crate::compiler::native_id::CHANNEL_IS_CLOSED => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelIsClosed)
        }
        crate::compiler::native_id::CHANNEL_LENGTH => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelLength)
        }
        crate::compiler::native_id::CHANNEL_CAPACITY => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelCapacity)
        }
        crate::compiler::native_id::MUTEX_TRY_LOCK => {
            BuiltinOp::HostHandle(HostHandleOpKind::MutexTryLock)
        }
        crate::compiler::native_id::MUTEX_IS_LOCKED => {
            BuiltinOp::HostHandle(HostHandleOpKind::MutexIsLocked)
        }
        crate::compiler::native_id::TASK_IS_DONE => {
            BuiltinOp::HostHandle(HostHandleOpKind::TaskIsDone)
        }
        crate::compiler::native_id::TASK_IS_CANCELLED => {
            BuiltinOp::HostHandle(HostHandleOpKind::TaskIsCancelled)
        }
        _ => return None,
    })
}

pub fn native_id_for_builtin_op(op: BuiltinOp) -> Option<u16> {
    Some(match op {
        BuiltinOp::Metaobject(kind) => match kind {
            MetaobjectOpKind::DefineProperty => crate::compiler::native_id::OBJECT_DEFINE_PROPERTY,
            MetaobjectOpKind::GetOwnPropertyDescriptor => {
                crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR
            }
            MetaobjectOpKind::DefineProperties => crate::compiler::native_id::OBJECT_DEFINE_PROPERTIES,
            MetaobjectOpKind::DeleteProperty => crate::compiler::native_id::OBJECT_DELETE_PROPERTY,
            MetaobjectOpKind::GetPrototypeOf => crate::compiler::native_id::OBJECT_GET_PROTOTYPE_OF,
            MetaobjectOpKind::SetPrototypeOf => crate::compiler::native_id::OBJECT_SET_PROTOTYPE_OF,
            MetaobjectOpKind::PreventExtensions => {
                crate::compiler::native_id::OBJECT_PREVENT_EXTENSIONS
            }
            MetaobjectOpKind::IsExtensible => crate::compiler::native_id::OBJECT_IS_EXTENSIBLE,
            MetaobjectOpKind::ReflectGet => crate::compiler::native_id::REFLECT_GET,
            MetaobjectOpKind::ReflectSet => crate::compiler::native_id::REFLECT_SET,
            MetaobjectOpKind::ReflectHas => crate::compiler::native_id::REFLECT_HAS,
            MetaobjectOpKind::ReflectOwnKeys => crate::compiler::native_id::REFLECT_OWN_KEYS,
            MetaobjectOpKind::ReflectConstruct => crate::compiler::native_id::REFLECT_CONSTRUCT,
        },
        BuiltinOp::Iterator(kind) => match kind {
            IteratorOpKind::GetIterator => crate::compiler::native_id::OBJECT_ITERATOR_GET,
            IteratorOpKind::GetAsyncIterator => crate::compiler::native_id::OBJECT_ASYNC_ITERATOR_GET,
            IteratorOpKind::Step => crate::compiler::native_id::OBJECT_ITERATOR_STEP,
            IteratorOpKind::Done => crate::compiler::native_id::OBJECT_ITERATOR_DONE,
            IteratorOpKind::Value => crate::compiler::native_id::OBJECT_ITERATOR_VALUE,
            IteratorOpKind::ResumeNext => crate::compiler::native_id::OBJECT_ITERATOR_RESUME_NEXT,
            IteratorOpKind::ResumeReturn => crate::compiler::native_id::OBJECT_ITERATOR_RESUME_RETURN,
            IteratorOpKind::ResumeThrow => crate::compiler::native_id::OBJECT_ITERATOR_RESUME_THROW,
            IteratorOpKind::Close => crate::compiler::native_id::OBJECT_ITERATOR_CLOSE,
            IteratorOpKind::CloseOnThrow => crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_ON_THROW,
            IteratorOpKind::CloseCompletion => {
                crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_COMPLETION
            }
            IteratorOpKind::AppendToArray => {
                crate::compiler::native_id::OBJECT_ITERATOR_APPEND_TO_ARRAY
            }
        },
        BuiltinOp::HostHandle(kind) => match kind {
            HostHandleOpKind::ChannelConstructor => crate::compiler::native_id::CHANNEL_NEW,
            HostHandleOpKind::ChannelSend => crate::compiler::native_id::CHANNEL_SEND,
            HostHandleOpKind::ChannelReceive => crate::compiler::native_id::CHANNEL_RECEIVE,
            HostHandleOpKind::ChannelTrySend => crate::compiler::native_id::CHANNEL_TRY_SEND,
            HostHandleOpKind::ChannelTryReceive => crate::compiler::native_id::CHANNEL_TRY_RECEIVE,
            HostHandleOpKind::ChannelClose => crate::compiler::native_id::CHANNEL_CLOSE,
            HostHandleOpKind::ChannelIsClosed => crate::compiler::native_id::CHANNEL_IS_CLOSED,
            HostHandleOpKind::ChannelLength => crate::compiler::native_id::CHANNEL_LENGTH,
            HostHandleOpKind::ChannelCapacity => crate::compiler::native_id::CHANNEL_CAPACITY,
            HostHandleOpKind::MutexTryLock => crate::compiler::native_id::MUTEX_TRY_LOCK,
            HostHandleOpKind::MutexIsLocked => crate::compiler::native_id::MUTEX_IS_LOCKED,
            HostHandleOpKind::TaskIsDone => crate::compiler::native_id::TASK_IS_DONE,
            HostHandleOpKind::TaskIsCancelled => crate::compiler::native_id::TASK_IS_CANCELLED,
            HostHandleOpKind::MutexConstructor
            | HostHandleOpKind::MutexLock
            | HostHandleOpKind::MutexUnlock
            | HostHandleOpKind::TaskCancel => return None,
        },
        BuiltinOp::Js(_) => return None,
    })
}
