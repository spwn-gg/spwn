//! TypeScript workflows: types are stripped before the module is loaded. There is no
//! type checking at run time — that's the editor's job, with `spwn.d.ts`.

use std::path::Path;

pub(crate) fn strip_types(_source: &str, path: &Path) -> Result<String, String> {
    Err(format!(
        "{}: TypeScript workflows aren't supported yet; use a .js file",
        path.display()
    ))
}
