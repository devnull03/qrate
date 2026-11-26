//! Luau is C++ that does not build for the browser, so a plugin there loads as a failure that says
//! why. Same surface as `plugin.rs`, so the registry and its callers compile unchanged.

use diagnostics::{ColumnFinding, ColumnInfo, ColumnValidator, ColumnValues};
use gpui::SharedString;
use plugin_api::{
    BarItem, ColumnMapSpec, CommandContext, ExportSnapshot, ExportSpec, MenuItem, SettingSpec,
};
use serde_json::Value as Json;

use crate::{Env, PackageDescriptor, Writes};

const DESKTOP_ONLY: &str = "Plugins run only in the qrate desktop app";

pub struct LuaPlugin {
    id: SharedString,
}

impl LuaPlugin {
    pub fn load(id: &str, _: &str, _: Env) -> Self {
        Self {
            id: id.to_owned().into(),
        }
    }

    pub fn load_packaged(
        id: &str,
        source: &str,
        env: Env,
        _: Result<Option<PackageDescriptor>, String>,
    ) -> Self {
        Self::load(id, source, env)
    }

    pub fn take_storage(&self) -> Option<Json> {
        None
    }

    pub fn id(&self) -> SharedString {
        self.id.clone()
    }

    pub fn permissions(&self) -> &[SharedString] {
        &[]
    }

    pub fn column_map(&self) -> Option<&ColumnMapSpec> {
        None
    }

    pub fn suggest(&self, _: &CommandContext) -> Result<Vec<SharedString>, String> {
        Err(DESKTOP_ONLY.into())
    }

    pub fn bar(&self) -> &[BarItem] {
        &[]
    }

    pub fn load_error(&self) -> Option<&str> {
        Some(DESKTOP_ONLY)
    }

    pub fn take_bar_updates(&self) -> Vec<(SharedString, SharedString)> {
        Vec::new()
    }

    pub fn description(&self) -> Option<SharedString> {
        None
    }

    pub fn menu(&self) -> &[MenuItem] {
        &[]
    }

    pub fn settings(&self) -> &[SettingSpec] {
        &[]
    }

    pub fn exports(&self) -> &[ExportSpec] {
        &[]
    }

    pub fn set_scoped(&self, _: Json, _: Json) {}

    pub fn set_app_settings(&self, _: SharedString) {}

    pub fn command(&self, _: &SharedString, _: &CommandContext) -> Result<Writes, String> {
        Err(DESKTOP_ONLY.into())
    }

    pub fn export(&self, _: &str, _: &ExportSnapshot) -> Result<Json, String> {
        Err(DESKTOP_ONLY.into())
    }
}

impl ColumnValidator for LuaPlugin {
    fn name(&self) -> SharedString {
        self.id.clone()
    }

    fn validate(&self, _: &ColumnInfo, _: ColumnValues<'_>) -> Vec<ColumnFinding> {
        Vec::new()
    }
}
