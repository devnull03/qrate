use gpui_component::setting::{SettingGroup, SettingPage};
use settings::Setting;

pub fn build_pages() -> Vec<SettingPage> {
    vec![
        SettingPage::new("Table").group(
            SettingGroup::new().title("Appearance").item(
                Setting::Switch {
                    key: table::TABLE_STRIPES_KEY,
                    label: "Row Stripes",
                    description: "Alternate row background color in the data table.",
                }
                .into(),
            ),
        ),
    ]
}
