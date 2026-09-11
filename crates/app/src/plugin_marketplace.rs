//! Signed plugin discovery and direct GitHub release installation.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::collapsible::Collapsible;
use gpui_component::input::{Input, InputEvent, InputState};
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
struct MarketplaceHandle(WeakEntity<MarketplaceWindow>);
impl Global for MarketplaceHandle {}

enum CatalogState {
    Loading,
    Ready(Vec<CatalogPlugin>),
    Error(Arc<str>),
}

enum DirectState {
    Idle,
    Loading,
    Review(Box<DirectReview>),
    Error(Arc<str>),
}

struct DirectReview {
    release: DirectRelease,
    inspection: PackageInspection,
    archive: PathBuf,
}

impl Drop for DirectReview {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.archive);
    }
}

pub(crate) struct MarketplaceWindow {
    catalog: CatalogState,
    direct: DirectState,
    input: Entity<InputState>,
    status: Option<Arc<str>>,
    requested_id: Option<String>,
    direct_expanded: bool,
    direct_request: u64,
    embedded: bool,
    _input_subscription: Subscription,
}

impl MarketplaceWindow {
    pub(crate) fn new(
        direct: bool,
        target: Option<InstallTarget>,
        embedded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        if !embedded {
            window.set_window_title("qrate plugins");
        }
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://github.com/owner/plugin"));
        let input_subscription = cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.status = None;
                this.resolve_direct(input.read(cx).value().to_string(), cx);
            }
        });
        let (requested_id, direct_source) = match target {
            Some(InstallTarget::Registry(id)) => (Some(id), None),
            Some(InstallTarget::Github(source)) => (None, Some(source)),
            None => (None, None),
        };
        let mut this = Self {
            catalog: CatalogState::Loading,
            direct: DirectState::Idle,
            input,
            status: None,
            requested_id,
            direct_expanded: direct || direct_source.is_some(),
            direct_request: 0,
            embedded,
            _input_subscription: input_subscription,
        };
        if this.requested_id.is_some() {
            this.refresh(cx);
        }
        if let Some(source) = direct_source {
            this.input
                .update(cx, |input, cx| input.set_value(source.clone(), window, cx));
            this.resolve_direct(source, cx);
        } else if direct {
            this.input.focus_handle(cx).focus(window, cx);
        }
        this
    }

    fn apply_target(&mut self, target: InstallTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.status = None;
        match target {
            InstallTarget::Registry(id) => {
                self.requested_id = Some(id);
                self.refresh(cx);
            }
            InstallTarget::Github(source) => {
                self.requested_id = None;
                self.direct_expanded = true;
                self.direct = DirectState::Idle;
                self.input
                    .update(cx, |input, cx| input.set_value(source.clone(), window, cx));
                self.resolve_direct(source, cx);
            }
        }
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.catalog = CatalogState::Loading;
        let key = option_env!("QRATE_PLUGIN_CATALOG_PUBLIC_KEY")
            .map(str::to_owned)
            .or_else(|| std::env::var("QRATE_PLUGIN_CATALOG_PUBLIC_KEY").ok());
        log::info!("fetching signed plugin catalog");
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let key = key.ok_or_else(|| {
                        anyhow::anyhow!(
                            "this build has no plugin catalog public key; set \
                             QRATE_PLUGIN_CATALOG_PUBLIC_KEY for local development"
                        )
                    })?;
                    let key = plugin_package::public_key(&key)?;
                    let plugins = plugin_host::plugins_dir()
                        .ok_or_else(|| anyhow::anyhow!("qrate application data is unavailable"))?;
                    let data = plugins
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("plugin folder has no parent"))?;
                    plugin_package::fetch_catalog_cached(
                        &crate::site::url("/plugins/catalog.json"),
                        &key,
                        &data.join("plugin-catalog"),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.catalog = match result {
                    Ok(catalog) => {
                        log::info!(
                            "signed plugin catalog loaded with {} listings",
                            catalog.plugins.len()
                        );
                        CatalogState::Ready(catalog.plugins)
                    }
                    Err(error) => {
                        log::warn!("signed plugin catalog is unavailable: {error:#}");
                        CatalogState::Error(format!("{error:#}").into())
                    }
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn resolve_direct(&mut self, source: String, cx: &mut Context<Self>) {
        self.direct_request = self.direct_request.wrapping_add(1);
        let request = self.direct_request;
        let source = source.trim().to_string();
        if source.trim().is_empty() {
            self.direct = DirectState::Idle;
            cx.notify();
            return;
        }
        if let Err(error) = plugin_package::validate_github_source(&source) {
            log::debug!("direct plugin source is not reviewable: {error:#}");
            self.direct = DirectState::Error(format!("{error:#}").into());
            cx.notify();
            return;
        }
        log::info!("checking direct plugin release from {source}");
        self.direct = DirectState::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            if !this
                .update(cx, |this, _| this.direct_request == request)
                .unwrap_or(false)
            {
                return;
            }
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
                    anyhow::Ok(DirectReview {
                        release,
                        inspection,
                        archive,
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                if this.direct_request != request {
                    return;
                }
                this.direct = match result {
                    Ok(review) => {
                        log::info!(
                            "direct plugin package checked: {} {} ({} bytes, SHA-256 {})",
                            review.inspection.manifest.id,
                            review.inspection.manifest.version,
                            review.inspection.bytes,
                            review.inspection.sha256
                        );
                        DirectState::Review(Box::new(review))
                    }
                    Err(error) => {
                        log::warn!("direct plugin package check failed: {error:#}");
                        DirectState::Error(format!("{error:#}").into())
                    }
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
            log::warn!("refused to install revoked plugin release {}", plugin.id);
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
                "Official catalog · {} · {permissions}.",
                plugin.publisher,
            )),
            &["Install", "Cancel"],
            cx,
        );
        let name = plugin.name.clone();
        cx.spawn_in(window, async move |this, cx| {
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
                    log::info!(
                        "installing official plugin {} {}",
                        plugin.id,
                        plugin.current.version
                    );
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
                    plugin_package::install_archive_checked(
                        temp.path(),
                        &plugins,
                        &receipts,
                        InstallSource::OfficialCatalog,
                        Some((&plugin, &plugin.current)),
                        |root, manifest| {
                            plugin_host::validate_package(root, &manifest.id)
                                .map_err(anyhow::Error::msg)
                        },
                    )
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(receipt) => {
                        log::info!(
                            "official plugin installed: {} {} (SHA-256 {})",
                            receipt.id,
                            receipt.version,
                            receipt.sha256
                        );
                        this.installed(&receipt, &name, already_managed, window, cx);
                    }
                    Err(error) => {
                        log::error!("official plugin installation failed: {error:#}");
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
        let DirectState::Review(review) = &self.direct else {
            return;
        };
        let DirectReview {
            release,
            inspection,
            archive,
        } = review.as_ref();
        let (release, inspection, archive) = (release.clone(), inspection.clone(), archive.clone());
        let permissions = if inspection.manifest.permissions.is_empty() {
            "No optional permissions".to_string()
        } else {
            format!("Requests: {}", inspection.manifest.permissions.join(", "))
        };
        let already_managed = installed_receipt(&inspection.manifest.id).is_some();
        let name = inspection.manifest.name.clone();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Install unlisted plugin {} {}?",
                inspection.manifest.name, inspection.manifest.version
            ),
            Some(&format!(
                "{} · SHA-256 {} · {permissions}. qrate has not reviewed this source.",
                release.repository, inspection.sha256,
            )),
            &["Install", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.unwrap_or(1) != 0 {
                return;
            }
            let result = cx
                .background_spawn(async move {
                    log::info!(
                        "installing direct plugin {} {} from {}",
                        inspection.manifest.id,
                        inspection.manifest.version,
                        release.repository
                    );
                    let plugins = plugin_host::plugins_dir()
                        .ok_or_else(|| anyhow::anyhow!("qrate application data is unavailable"))?;
                    let data = plugins
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("plugin folder has no parent"))?;
                    let receipts = plugin_package::receipts_dir(data);
                    let result = plugin_package::install_direct_archive_checked(
                        &archive,
                        &plugins,
                        &receipts,
                        &release,
                        |root, manifest| {
                            plugin_host::validate_package(root, &manifest.id)
                                .map_err(anyhow::Error::msg)
                        },
                    );
                    let _ = std::fs::remove_file(archive);
                    result
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(receipt) => {
                        log::info!(
                            "direct plugin installed: {} {} (SHA-256 {})",
                            receipt.id,
                            receipt.version,
                            receipt.sha256
                        );
                        this.direct = DirectState::Idle;
                        this.installed(&receipt, &name, already_managed, window, cx);
                    }
                    Err(error) => {
                        log::error!("direct plugin installation failed: {error:#}");
                        this.status = Some(format!("Installation failed: {error:#}").into())
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn installed(
        &mut self,
        receipt: &plugin_package::InstallReceipt,
        name: &str,
        already_managed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Removing a package switches it off, so a reinstall has to switch it back on.
        if !already_managed {
            settings::plugins::set_enabled(&receipt.id, true, cx);
        }
        plugin_host::reload(cx);
        if self.embedded {
            self.status = Some(format!("{name} {} installed and enabled.", receipt.version).into());
        } else {
            window.remove_window();
        }
    }
}

impl Render for MarketplaceWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let permissions = |permissions: &[String]| {
            if permissions.is_empty() {
                "No optional permissions".to_string()
            } else {
                format!("Requests {}", permissions.join(", "))
            }
        };
        let requested = match &self.catalog {
            CatalogState::Ready(plugins) => self
                .requested_id
                .as_ref()
                .and_then(|id| plugins.iter().find(|plugin| &plugin.id == id))
                .cloned(),
            _ => None,
        };

        let catalog = match (&self.catalog, &requested) {
            (_, Some(plugin)) => v_flex()
                .gap_1()
                .child(
                    Label::new(format!("{} {}", plugin.name, plugin.current.version))
                        .font_semibold(),
                )
                .child(Label::new(plugin.summary.clone()).text_sm())
                .child(
                    Label::new(format!(
                        "{} · {} · API {} · {}",
                        plugin.publisher,
                        plugin.license,
                        plugin.current.api_version,
                        permissions(&plugin.current.permissions)
                    ))
                    .text_sm()
                    .text_color(muted),
                )
                .child(
                    Label::new(plugin.repository.clone())
                        .text_xs()
                        .text_color(muted),
                )
                .into_any_element(),
            (CatalogState::Loading, _) => Label::new("Loading the signed catalog…")
                .text_sm()
                .into_any_element(),
            (CatalogState::Error(error), _) => Label::new(format!("Catalog unavailable: {error}"))
                .text_sm()
                .text_color(muted)
                .into_any_element(),
            (CatalogState::Ready(_), None) => {
                Label::new("That plugin is not in the current signed catalog.")
                    .text_sm()
                    .text_color(muted)
                    .into_any_element()
            }
        };

        let direct = match &self.direct {
            DirectState::Idle => Label::new(
                "Paste a public GitHub repository or release URL. qrate checks valid links automatically.",
            )
            .text_sm()
            .text_color(muted)
            .into_any_element(),
            DirectState::Loading => Label::new("Resolving and checking release…")
                .text_sm()
                .into_any_element(),
            DirectState::Error(error) => Label::new(format!("Could not review package: {error}"))
                .text_sm()
                .text_color(cx.theme().danger_foreground)
                .into_any_element(),
            DirectState::Review(review) => {
                let manifest = &review.inspection.manifest;
                v_flex()
                    .gap_1()
                    .child(
                        Label::new(format!("{} {}", manifest.name, manifest.version))
                            .font_semibold(),
                    )
                    .child(
                        Label::new(format!(
                            "API {} · {} · {} bytes · {}",
                            manifest.api_version,
                            manifest.license,
                            review.inspection.bytes,
                            permissions(&manifest.permissions)
                        ))
                        .text_sm()
                        .text_color(muted),
                    )
                    .child(
                        Label::new(review.release.repository.clone())
                            .text_xs()
                            .text_color(muted),
                    )
                    .child(
                        Label::new(format!("SHA-256 {}", review.inspection.sha256))
                            .text_xs()
                            .text_color(muted),
                    )
                    .into_any_element()
            }
        };

        let action = if self.requested_id.is_some() {
            match (&self.catalog, requested) {
                (_, Some(plugin)) => {
                    let installed = installed_receipt(&plugin.id);
                    let revoked = plugin.current.status == ReleaseStatus::Revoked;
                    let current = installed
                        .as_ref()
                        .is_some_and(|receipt| receipt.version >= plugin.current.version);
                    Some(
                        Button::new("install-official")
                            .primary()
                            .label(if revoked {
                                "Revoked"
                            } else if current {
                                "Installed"
                            } else if installed.is_some() {
                                "Update"
                            } else {
                                "Install"
                            })
                            .disabled(revoked || current)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.install_official(plugin.clone(), window, cx);
                            })),
                    )
                }
                (CatalogState::Error(_), None) => Some(
                    Button::new("catalog-site")
                        .label("Browse website")
                        .on_click(|_, _, cx| open_catalog(cx)),
                ),
                _ => None,
            }
        } else if matches!(self.direct, DirectState::Review(_)) {
            Some(
                Button::new("install-direct")
                    .primary()
                    .label("Install unlisted plugin")
                    .on_click(cx.listener(|this, _, window, cx| this.install_direct(window, cx))),
            )
        } else {
            None
        };
        let (inline_action, footer_action) = if self.embedded {
            (action, None)
        } else {
            (None, action)
        };
        let status = self
            .status
            .clone()
            .map(|status| Label::new(status).text_sm().text_color(muted));

        let direct_installer = Collapsible::new()
            .open(self.direct_expanded)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("browse-plugin-catalog")
                            .small()
                            .label("Browse catalog")
                            .on_click(|_, _, cx| open_catalog(cx)),
                    )
                    .child(
                        Button::new("toggle-github-installer")
                            .small()
                            .label(if self.direct_expanded {
                                "Hide GitHub installer"
                            } else {
                                "Install from GitHub…"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.direct_expanded = !this.direct_expanded;
                                if this.direct_expanded {
                                    this.input.focus_handle(cx).focus(window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .content(
                v_flex()
                    .gap_2()
                    .mt_2()
                    .child(Input::new(&self.input).w_full())
                    .child(direct)
                    .children(inline_action.map(|action| h_flex().child(action.small()))),
            );

        if self.embedded {
            return v_flex()
                .gap_2()
                .child(direct_installer)
                .children(status)
                .into_any_element();
        }

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
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_4()
                    .gap_3()
                    .map(|body| {
                        if self.requested_id.is_some() {
                            body.child(catalog)
                        } else {
                            body.child(direct_installer)
                        }
                    })
                    .children(status),
            )
            .when_some(footer_action, |view, action| {
                view.child(
                    h_flex()
                        .justify_end()
                        .px_4()
                        .py_3()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(action),
                )
            })
            .into_any_element()
    }
}

fn installed_receipt(id: &str) -> Option<plugin_package::InstallReceipt> {
    plugin_host::plugins_dir().and_then(|plugins| {
        let data = plugins.parent()?;
        plugin_package::read_receipt(&plugin_package::receipts_dir(data), id)
            .ok()
            .flatten()
    })
}

pub fn open_catalog(cx: &mut gpui::App) {
    log::info!("opening plugin catalog in the default browser");
    cx.open_url(&crate::site::url("/plugins"));
}

pub fn open_install_target(target: InstallTarget, cx: &mut gpui::App) {
    open_marketplace(matches!(target, InstallTarget::Github(_)), Some(target), cx);
}

fn open_marketplace(direct: bool, target: Option<InstallTarget>, cx: &mut gpui::App) {
    if let Some(handle) = WindowRegistry::focus_or_clear(MARKETPLACE_WINDOW_KIND, cx) {
        if let Some(target) = target {
            handle
                .update(cx, |_, window, cx| {
                    if let Some(marketplace) = cx
                        .try_global::<MarketplaceHandle>()
                        .and_then(|handle| handle.0.upgrade())
                    {
                        marketplace.update(cx, |marketplace, cx| {
                            marketplace.apply_target(target, window, cx);
                        });
                    }
                })
                .ok();
        }
        return;
    }
    let win_size = size(px(460.0), px(380.0));
    let options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, win_size, cx))),
        window_min_size: Some(size(px(400.0), px(300.0))),
        ..Default::default()
    };
    if let Ok(handle) = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| MarketplaceWindow::new(direct, target, false, window, cx));
        cx.set_global(MarketplaceHandle(view.downgrade()));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        WindowRegistry::register(MARKETPLACE_WINDOW_KIND, handle.into(), cx);
    }
}
