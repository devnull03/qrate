//! Signed plugin discovery and direct GitHub release installation.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::Button;
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Root, Sizable as _, StyledExt as _, TitleBar, h_flex,
    label::Label, v_flex,
};
use plugin_package::{
    CatalogPlugin, DirectRelease, InstallSource, InstallTarget, PackageInspection, ReleaseStatus,
};
use window_wrapper::WindowRegistry;

const MARKETPLACE_WINDOW_KIND: &str = "plugin-marketplace";
const SITE_URL: &str = "https://qrate.dvnl.work/plugins";

enum CatalogState {
    Loading,
    Ready(Vec<CatalogPlugin>),
    Error(Arc<str>),
}

enum DirectState {
    Idle,
    Loading,
    Review {
        release: DirectRelease,
        inspection: PackageInspection,
        archive: PathBuf,
    },
    Error(Arc<str>),
}

pub struct MarketplaceWindow {
    catalog: CatalogState,
    direct: DirectState,
    input: Entity<InputState>,
    status: Option<Arc<str>>,
    requested_id: Option<String>,
    direct_source: Option<String>,
}

impl MarketplaceWindow {
    fn new(
        direct: bool,
        target: Option<InstallTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        window.set_window_title("qrate plugins");
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://github.com/owner/plugin"));
        let (requested_id, direct_source) = match target {
            Some(InstallTarget::Registry(id)) => (Some(id), None),
            Some(InstallTarget::Github(source)) => (None, Some(source)),
            None => (None, None),
        };
        let mut this = Self {
            catalog: CatalogState::Loading,
            direct: DirectState::Idle,
            input,
            status: requested_id
                .as_ref()
                .map(|id| format!("Reviewing official catalog entry {id}").into()),
            requested_id,
            direct_source,
        };
        this.refresh(cx);
        if direct && this.direct_source.is_none() {
            this.input.focus_handle(cx).focus(window, cx);
        }
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.catalog = CatalogState::Loading;
        let key = option_env!("QRATE_PLUGIN_CATALOG_PUBLIC_KEY")
            .map(str::to_owned)
            .or_else(|| std::env::var("QRATE_PLUGIN_CATALOG_PUBLIC_KEY").ok());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let key = key.ok_or_else(|| {
                        anyhow::anyhow!("this build has no plugin catalog public key")
                    })?;
                    let key = plugin_package::public_key(&key)?;
                    let plugins = plugin_host::plugins_dir()
                        .ok_or_else(|| anyhow::anyhow!("qrate application data is unavailable"))?;
                    let data = plugins
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("plugin folder has no parent"))?;
                    plugin_package::fetch_catalog_cached(
                        plugin_package::CATALOG_URL,
                        &key,
                        &data.join("plugin-catalog"),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.catalog = match result {
                    Ok(catalog) => CatalogState::Ready(catalog.plugins),
                    Err(error) => CatalogState::Error(format!("{error:#}").into()),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn resolve_direct(&mut self, cx: &mut Context<Self>) {
        let source = self
            .direct_source
            .clone()
            .unwrap_or_else(|| self.input.read(cx).value().to_string());
        if source.trim().is_empty() {
            self.direct =
                DirectState::Error("Paste a public GitHub repository or release URL".into());
            cx.notify();
            return;
        }
        self.direct = DirectState::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let release = plugin_package::resolve_github_release(&source)?;
                    let temp = tempfile::Builder::new()
                        .prefix("qrate-plugin-download-")
                        .suffix(".zip")
                        .tempfile()?;
                    plugin_package::download_package(&release.artifact_url, temp.path())?;
                    let inspection = plugin_package::inspect_archive(temp.path())?;
                    let archive = temp.into_temp_path().keep()?;
                    anyhow::Ok((release, inspection, archive))
                })
                .await;
            this.update(cx, |this, cx| {
                this.direct = match result {
                    Ok((release, inspection, archive)) => DirectState::Review {
                        release,
                        inspection,
                        archive,
                    },
                    Err(error) => DirectState::Error(format!("{error:#}").into()),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn install_official(
        &mut self,
        plugin: CatalogPlugin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if plugin.current.status == ReleaseStatus::Revoked {
            self.status = Some("This release is revoked and cannot be installed".into());
            cx.notify();
            return;
        }
        let already_managed = plugin_host::plugins_dir().is_some_and(|plugins| {
            plugins.parent().is_some_and(|data| {
                plugin_package::read_receipt(&plugin_package::receipts_dir(data), &plugin.id)
                    .ok()
                    .flatten()
                    .is_some()
            })
        });
        let permissions = if plugin.current.permissions.is_empty() {
            "No optional permissions".to_string()
        } else {
            format!("Requests: {}", plugin.current.permissions.join(", "))
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Install {} {}?", plugin.name, plugin.current.version),
            Some(&format!(
                "Official catalog · {} · {permissions}. The plugin will be disabled after installation.",
                plugin.publisher
            )),
            &["Install", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.unwrap_or(1) != 0 {
                return;
            }
            this.update(cx, |this, cx| {
                this.status = Some("Downloading and checking package…".into());
                cx.notify();
            })
            .ok();
            let result = cx
                .background_spawn(async move {
                    let temp = tempfile::Builder::new()
                        .prefix("qrate-plugin-download-")
                        .suffix(".zip")
                        .tempfile()?;
                    plugin_package::download_package(&plugin.current.artifact_url, temp.path())?;
                    let plugins = plugin_host::plugins_dir()
                        .ok_or_else(|| anyhow::anyhow!("qrate application data is unavailable"))?;
                    let data = plugins
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("plugin folder has no parent"))?;
                    let receipts = plugin_package::receipts_dir(data);
                    plugin_package::install_archive(
                        temp.path(),
                        &plugins,
                        &receipts,
                        InstallSource::OfficialCatalog,
                        Some((&plugin, &plugin.current)),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(receipt) => {
                        if !already_managed {
                            settings::plugins::set_enabled(&receipt.id, false, cx);
                        }
                        plugin_host::reload(cx);
                        this.status = Some(
                            format!(
                                "{} {} installed{}. Manage it in Settings.",
                                receipt.id,
                                receipt.version,
                                if already_managed { "" } else { " disabled" }
                            )
                            .into(),
                        );
                    }
                    Err(error) => {
                        this.status = Some(format!("Installation failed: {error:#}").into())
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn install_direct(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let DirectState::Review {
            release,
            inspection,
            archive,
        } = &self.direct
        else {
            return;
        };
        let (release, inspection, archive) = (release.clone(), inspection.clone(), archive.clone());
        let permissions = if inspection.manifest.permissions.is_empty() {
            "No optional permissions".to_string()
        } else {
            format!("Requests: {}", inspection.manifest.permissions.join(", "))
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Install unlisted plugin {} {}?",
                inspection.manifest.name, inspection.manifest.version
            ),
            Some(&format!(
                "{} · SHA-256 {} · {permissions}. qrate has not reviewed this source.",
                release.repository, inspection.sha256
            )),
            &["Install", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.unwrap_or(1) != 0 {
                return;
            }
            let result = cx
                .background_spawn(async move {
                    let plugins = plugin_host::plugins_dir()
                        .ok_or_else(|| anyhow::anyhow!("qrate application data is unavailable"))?;
                    let data = plugins
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("plugin folder has no parent"))?;
                    let receipts = plugin_package::receipts_dir(data);
                    let already_managed =
                        plugin_package::read_receipt(&receipts, &inspection.manifest.id)?.is_some();
                    let result = plugin_package::install_direct_archive(
                        &archive, &plugins, &receipts, &release,
                    );
                    let _ = std::fs::remove_file(archive);
                    result.map(|receipt| (receipt, already_managed))
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((receipt, already_managed)) => {
                        if !already_managed {
                            settings::plugins::set_enabled(&receipt.id, false, cx);
                        }
                        plugin_host::reload(cx);
                        this.status = Some(
                            format!(
                                "{} {} installed{}. Manage it in Settings.",
                                receipt.id,
                                receipt.version,
                                if already_managed { "" } else { " disabled" }
                            )
                            .into(),
                        );
                        this.direct = DirectState::Idle;
                    }
                    Err(error) => {
                        this.status = Some(format!("Installation failed: {error:#}").into())
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for MarketplaceWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let catalog = match &self.catalog {
            CatalogState::Loading => Label::new("Loading the signed catalog…")
                .text_sm()
                .into_any_element(),
            CatalogState::Error(error) => h_flex()
                .gap_2()
                .child(
                    Label::new(format!("Catalog unavailable: {error}"))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                )
                .child(
                    Button::new("catalog-site")
                        .label("Browse website")
                        .small()
                        .on_click(|_, _, cx| cx.open_url(SITE_URL)),
                )
                .into_any_element(),
            CatalogState::Ready(plugins) if plugins.is_empty() => {
                Label::new("The official catalog has no plugins yet.")
                    .text_sm()
                    .into_any_element()
            }
            CatalogState::Ready(plugins) => v_flex()
                .gap_2()
                .children(
                    plugins
                        .iter()
                        .filter(|plugin| {
                            self.requested_id.as_ref().is_none_or(|id| id == &plugin.id)
                        })
                        .cloned()
                        .map(|plugin| {
                            let button_plugin = plugin.clone();
                            h_flex()
                                .justify_between()
                                .gap_3()
                                .p_3()
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(
                                    v_flex()
                                        .child(Label::new(plugin.name).font_semibold())
                                        .child(
                                            Label::new(format!(
                                                "{} · {} · API {}",
                                                plugin.summary,
                                                plugin.current.version,
                                                plugin.current.api_version
                                            ))
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground),
                                        ),
                                )
                                .child(
                                    Button::new(format!("install-{}", plugin.id))
                                        .label(if plugin.current.status == ReleaseStatus::Revoked {
                                            "Revoked"
                                        } else {
                                            "Install"
                                        })
                                        .small()
                                        .disabled(plugin.current.status == ReleaseStatus::Revoked)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.install_official(
                                                button_plugin.clone(),
                                                window,
                                                cx,
                                            );
                                        })),
                                )
                        }),
                )
                .into_any_element(),
        };

        let direct = match &self.direct {
            DirectState::Idle => Label::new(
                "Direct installs are unlisted. qrate downloads the package before it shows the final review.",
            )
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .into_any_element(),
            DirectState::Loading => Label::new("Resolving and checking release…")
                .text_sm()
                .into_any_element(),
            DirectState::Error(error) => Label::new(format!("Could not review package: {error}"))
                .text_sm()
                .text_color(cx.theme().danger_foreground)
                .into_any_element(),
            DirectState::Review {
                release,
                inspection,
                ..
            } => h_flex()
                .justify_between()
                .gap_3()
                .child(
                    Label::new(format!(
                        "{} {} · {} bytes · SHA-256 {} · {}",
                        inspection.manifest.name,
                        inspection.manifest.version,
                        inspection.bytes,
                        inspection.sha256,
                        release.repository
                    ))
                    .text_sm(),
                )
                .child(
                    Button::new("install-direct")
                        .label("Install unlisted plugin")
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.install_direct(window, cx);
                        })),
                )
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .child(
                TitleBar::new()
                    .text_xs()
                    .text_color(cx.theme().foreground)
                    .child(Label::new("qrate plugins").font_semibold()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .p_4()
                    .gap_4()
                    .child(Label::new("Official catalog").text_lg().font_semibold())
                    .child(catalog)
                    .child(Label::new("Install from GitHub").text_lg().font_semibold())
                    .when_some(self.direct_source.clone(), |view, source| {
                        view.child(
                            Label::new(format!("Requested source: {source}"))
                                .text_sm()
                                .text_color(cx.theme().muted_foreground),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.input)))
                            .child(
                                Button::new("review-direct")
                                    .label("Review package")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.resolve_direct(cx);
                                    })),
                            ),
                    )
                    .child(direct)
                    .when_some(self.status.clone(), |view, status| {
                        view.child(
                            Label::new(status)
                                .text_sm()
                                .text_color(cx.theme().muted_foreground),
                        )
                    }),
            )
    }
}

pub fn open_marketplace_window(direct: bool, cx: &mut gpui::App) {
    open_marketplace(direct, None, cx);
}

pub fn open_install_target(target: InstallTarget, cx: &mut gpui::App) {
    open_marketplace(matches!(target, InstallTarget::Github(_)), Some(target), cx);
}

fn open_marketplace(direct: bool, target: Option<InstallTarget>, cx: &mut gpui::App) {
    if WindowRegistry::focus_or_clear(MARKETPLACE_WINDOW_KIND, cx).is_some() {
        return;
    }
    let win_size = size(px(760.0), px(620.0));
    let options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, win_size, cx))),
        window_min_size: Some(size(px(560.0), px(420.0))),
        ..Default::default()
    };
    if let Ok(handle) = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| MarketplaceWindow::new(direct, target, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        WindowRegistry::register(MARKETPLACE_WINDOW_KIND, handle.into(), cx);
    }
}
