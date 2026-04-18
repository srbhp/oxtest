use std::sync::{Arc, Mutex, OnceLock};
use super::types::{RunConfig, TestItem, TestResult};

static PLUGIN_REGISTRY: OnceLock<Mutex<Vec<Arc<dyn OxtestPlugin>>>> = OnceLock::new();

pub(crate) fn plugin_registry() -> &'static Mutex<Vec<Arc<dyn OxtestPlugin>>> {
    PLUGIN_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

pub trait OxtestPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn configure(&self, _config: &mut RunConfig) {}
    fn collect(&self, _tests: &mut Vec<TestItem>) {}
    fn before_test(&self, _test: &TestItem) {}
    fn after_test(&self, _test: &TestItem, _result: &TestResult) {}
}

pub fn register_plugin(plugin: Arc<dyn OxtestPlugin>) {
    plugin_registry().lock().unwrap().push(plugin);
}
