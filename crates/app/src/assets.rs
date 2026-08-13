//! Our own icons in front of `gpui_component_assets`, which is otherwise the only asset source.
//! Lucide ships no filled panel glyph, so the title bar's "this dock is open" state needs three
//! SVGs of our own.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct Assets;

const OWN: [(&str, &str); 3] = [
    (
        "icons/panel-left-filled.svg",
        include_str!("../../../assets/icons/panel-left-filled.svg"),
    ),
    (
        "icons/panel-right-filled.svg",
        include_str!("../../../assets/icons/panel-right-filled.svg"),
    ),
    (
        "icons/panel-bottom-filled.svg",
        include_str!("../../../assets/icons/panel-bottom-filled.svg"),
    ),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match OWN.iter().find(|(name, _)| *name == path) {
            Some((_, svg)) => Ok(Some(Cow::Borrowed(svg.as_bytes()))),
            None => gpui_component_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = gpui_component_assets::Assets.list(path)?;
        all.extend(
            OWN.iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use gpui::AssetSource as _;

    #[test]
    fn our_icons_load_and_the_library_still_does() {
        let assets = super::Assets;
        assert!(
            assets
                .load("icons/panel-left-filled.svg")
                .unwrap()
                .is_some_and(|svg| svg.starts_with(b"<svg"))
        );
        assert!(assets.load("icons/panel-left.svg").unwrap().is_some());
    }
}
