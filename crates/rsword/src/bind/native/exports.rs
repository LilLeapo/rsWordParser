//! `BIND-01`：原生会话与语言外壳共用的导出表；替代旧 `wasm_export!`。

/// 同表展开导出与错误合同测试。外壳只转换语言类型，会话面统一先检查句柄。
#[macro_export]
macro_rules! bind_export {
    (class [$attr:meta] error($map:path, $error:ty); $class:ident($field:ident); $(
        $(#[$doc:meta])*
        $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty => $method:ident($($pass:expr),*);
    )*) => {
        #[$attr]
        impl $class {$(
            $(#[$doc])*
            pub fn $name(&mut self, $($arg:$ty),*) -> Result<$ret,$error> {
                self.$field.$method($($pass),*).map_err($map)
            }
        )*}
    };
    (adapter [$attr:meta] error($map:path, $error:ty); $(
        $(#[doc = $doc:expr])*
        $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty => $core:path
        { $(test $test:ident {$($body:tt)*})* } $(,)?
    )+) => {$(
        $(#[doc = $doc])*  #[$attr]
        pub fn $name($($arg: $ty),*) -> Result<$ret, $error> {
            $core($($arg),*).map_err($map)
        }
        $(#[cfg(test)] #[test] fn $test() {$($body)*})*
    )+};
    (sessions $table:ty; $(
        $(#[doc = $doc:expr])*
        $name:ident($($arg:ident: $ty:ty = $sample:expr),*) -> $ret:ty => $core:ident,
        test $test:ident;
    )+) => {
        #[cfg_attr(rsword_api_docs, deny(missing_docs))]
        impl $table {$(
            $(#[doc = $doc])*
            pub fn $name(&mut self, id: &str, $($arg: $ty),*) -> Result<$ret, $crate::bind::native::ApiError> {
                self.require_session(id)?;
                self.$core(id, $($arg),*)
            }
        )+}
            $(#[test] fn $test() {
                let mut table = <$table>::default();
                assert_eq!(table.$name("s-missing", $($sample),*).unwrap_err().code, "BIND_NO_SESSION");
            })+
    };
}
