/// What a column's values are shaped like, as declared in `__columns.data_type`.
///
/// The stored value stays free-form `TEXT` — this only *interprets* it, so a project written by
/// hand or by an older build still loads and a type nobody here knows reads as [`Self::Text`]
/// rather than becoming an error. Validators key off this instead of matching type strings
/// themselves, which is what stops each one growing its own spelling list.
///
/// What a column is checked against is a separate project setting: a subject heading can be
/// `Text` while also requiring an authority lookup.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ColumnType {
    #[default]
    Text,
    /// The primary human-readable name of a row.
    Title,
    /// A date, validated as EDTF.
    Date,
    /// Names a file in the project's files folder.
    Filename,
    Number,
    Url,
    /// An accession number, call number, or other opaque handle.
    Identifier,
    /// The archival level assigned to this component.
    DescriptionLevel,
    /// A stable reference to this component's broader parent.
    ParentComponent,
    /// A file or directory path retained from filesystem ingest.
    SourcePath,
}

impl ColumnType {
    /// Canonical spelling — what the wizard writes and `column_config.csv` should say.
    pub fn as_str(self) -> &'static str {
        match self {
            ColumnType::Title => "Title",
            ColumnType::Text => "Text",
            ColumnType::Date => "Date",
            ColumnType::Filename => "Filename",
            ColumnType::Number => "Number",
            ColumnType::Url => "Url",
            ColumnType::Identifier => "Identifier",
            ColumnType::DescriptionLevel => "Description Level",
            ColumnType::ParentComponent => "Parent Component",
            ColumnType::SourcePath => "Source Path",
        }
    }

    /// Every type, in the order the wizard offers them.
    pub const ALL: [ColumnType; 10] = [
        ColumnType::Title,
        ColumnType::Text,
        ColumnType::Date,
        ColumnType::Filename,
        ColumnType::Number,
        ColumnType::Url,
        ColumnType::Identifier,
        ColumnType::DescriptionLevel,
        ColumnType::ParentComponent,
        ColumnType::SourcePath,
    ];

    /// Read a declared type. Case- and space-insensitive, and it accepts the spellings a person
    /// writing a CSV by hand actually uses (`int`, `datetime`, `uri`) so a config isn't rejected
    /// over a synonym. Anything unrecognised is [`Self::Text`], the type that assumes least.
    pub fn from_declared(declared: &str) -> Self {
        match declared.trim().to_ascii_lowercase().as_str() {
            "title" => ColumnType::Title,
            "date" | "datetime" | "time" | "year" | "edtf" => ColumnType::Date,
            "filename" | "file" | "filepath" | "path" => ColumnType::Filename,
            "number" | "integer" | "int" | "float" | "decimal" => ColumnType::Number,
            "url" | "uri" | "link" => ColumnType::Url,
            "identifier" | "id" => ColumnType::Identifier,
            "descriptionlevel" | "description level" | "levelofdescription" => {
                ColumnType::DescriptionLevel
            }
            "parentcomponent" | "parent component" | "parent" => ColumnType::ParentComponent,
            "sourcepath" | "source path" => ColumnType::SourcePath,
            _ => ColumnType::Text,
        }
    }

    /// Whether cells hold prose — the question spell checking and any other language-aware rule
    /// asks.
    pub fn is_prose(self) -> bool {
        matches!(self, ColumnType::Title | ColumnType::Text)
    }
}
