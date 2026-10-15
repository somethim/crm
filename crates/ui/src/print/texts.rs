//! Albanian texts of the receipt preview (scanned by the English denylist).

/// The preview window title (also finds its native window on Windows).
pub const PREVIEW_TITLE: &str = "Parapamja e printimit";
/// Tooltip of the disabled "Printo…" outside Windows.
pub const PRINT_ONLY_ON_WINDOWS: &str = "Printimi është i disponueshëm vetëm në Windows";
/// The receipt could not be prepared.
pub const PREVIEW_FAILED: &str = "Parapamja nuk u krijua: {arsyeja}";
/// Printing failed (the reason follows).
pub const PRINT_FAILED: &str = "Printimi dështoi: {arsyeja}";
/// After the receipt went to the printer.
pub const PRINTED: &str = "Porosia u dërgua te printeri.";
/// The save dialog title.
pub const SAVE_TITLE: &str = "Ruaj porosinë si foto";
/// The save dialog filter name.
pub const PNG_FILTER: &str = "Foto PNG";
/// After the PNG was written.
pub const SAVED: &str = "Fotoja u ruajt: {skedari}";
/// Writing the PNG failed.
pub const SAVE_FAILED: &str = "Ruajtja e fotos dështoi: {arsyeja}";
/// The document name in the print queue.
pub const DOC_NAME: &str = "Porosia {nr}";

/// The rendered document had no page.
pub const EMPTY_PAGE: &str = "porosia nuk ka asnjë faqe";

// The "{arsyeja}" sentences (F-024): the raw typst / OS text goes to the log
// only; the user sees one of these.

/// The receipt is taller than the guard (`{mm}` millimetres).
pub const REASON_TOO_TALL: &str = "porosia është shumë e gjatë për printim ({mm} mm)";
/// The page is too large to draw (an out-of-range width or resolution).
pub const REASON_TOO_LARGE: &str =
    "porosia është shumë e madhe për t'u vizatuar; kontrolloni gjerësinë e letrës te Cilësimet";
/// The template or the PNG encoder failed.
pub const REASON_NOT_PREPARED: &str = "porosia nuk u përgatit";
/// No permission to write there (or a read-only disk).
pub const REASON_NO_WRITE_PERMISSION: &str = "nuk ka leje për të shkruar në këtë vend";
/// The disk is full.
pub const REASON_DISK_FULL: &str = "disku është plot";
/// The folder does not exist (any more).
pub const REASON_FOLDER_NOT_FOUND: &str = "dosja nuk u gjet";
/// The disk is gone or not ready (a removed USB stick).
pub const REASON_DISK_NOT_READY: &str = "disku nuk është i gatshëm (a u hoq USB-ja?)";
/// A folder already has that name.
pub const REASON_IS_FOLDER: &str = "ekziston një dosje me këtë emër";
/// Any other write failure.
pub const REASON_NOT_WRITTEN: &str = "skedari nuk u shkrua";
/// The program window for the print dialog was not found.
pub const REASON_NO_WINDOW: &str = "dritarja e programit nuk u gjet";
/// The printer could not be opened.
pub const REASON_NO_PRINTER: &str = "printeri nuk u hap";
/// The printer reported no resolution or printable area.
pub const REASON_NO_RESOLUTION: &str =
    "printeri nuk tregoi rezolucionin ose sipërfaqen e printimit";
/// No permission to use the printer.
pub const REASON_PRINTER_DENIED: &str = "nuk ka leje për të përdorur këtë printer";
/// Out of memory while printing.
pub const REASON_NO_MEMORY: &str = "nuk ka memorie të mjaftueshme";
/// The job was deleted from the print queue.
pub const REASON_JOB_CANCELLED: &str = "printimi u anulua nga radha e printerit";
/// The print service does not answer (stopped spooler).
pub const REASON_SPOOLER: &str = "shërbimi i printimit në Windows nuk përgjigjet";
/// The print dialog failed.
pub const REASON_DIALOG: &str = "dialogu i printimit nuk u hap";
/// `StartDoc` failed.
pub const REASON_START_DOC: &str = "printeri nuk e pranoi dokumentin";
/// `StartPage` / `EndPage` failed.
pub const REASON_PAGE: &str = "printeri nuk pranoi një faqe";
/// `StretchDIBits` failed.
pub const REASON_SEND_IMAGE: &str = "figura nuk u dërgua te printeri";
/// `EndDoc` failed.
pub const REASON_END_DOC: &str = "dokumenti nuk u mbyll te printeri";

/// The Albanian reason of a rendering failure.
pub fn render_reason(e: &crm_reports::RenderError) -> String {
    use crm_reports::RenderError;
    match e {
        RenderError::TooTall(mm) => fill(REASON_TOO_TALL, "mm", &mm.to_string()),
        RenderError::TooLarge { .. } => REASON_TOO_LARGE.to_owned(),
        RenderError::Template(_) | RenderError::Png(_) => REASON_NOT_PREPARED.to_owned(),
    }
}

/// The Albanian reason of a failed file write.
pub fn io_reason(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind;
    // ERROR_NOT_READY / ERROR_DEV_NOT_EXIST: the drive went away.
    if cfg!(windows) && matches!(e.raw_os_error(), Some(21 | 55)) {
        return REASON_DISK_NOT_READY;
    }
    match e.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => REASON_NO_WRITE_PERMISSION,
        ErrorKind::StorageFull => REASON_DISK_FULL,
        ErrorKind::NotFound => REASON_FOLDER_NOT_FOUND,
        ErrorKind::IsADirectory => REASON_IS_FOLDER,
        _ => REASON_NOT_WRITTEN,
    }
}

/// The Albanian reason of a printing failure.
pub fn print_reason(e: &crm_reports::PrintError) -> String {
    use crm_reports::{PrintError, PrintStep};
    let reason = match e {
        PrintError::NoWindow => REASON_NO_WINDOW,
        PrintError::NoPrinter => REASON_NO_PRINTER,
        PrintError::NoResolution => REASON_NO_RESOLUTION,
        PrintError::Render(r) => return render_reason(r),
        PrintError::Os { step, code } => match (step, code) {
            (PrintStep::Dialog, _) => REASON_DIALOG,
            // Win32 codes: access denied, not enough memory / out of memory,
            // disk full, job deleted from the queue, RPC server (spooler)
            // unavailable.
            (_, Some(5)) => REASON_PRINTER_DENIED,
            (_, Some(8 | 14)) => REASON_NO_MEMORY,
            (_, Some(39 | 112)) => REASON_DISK_FULL,
            (_, Some(63)) => REASON_JOB_CANCELLED,
            (_, Some(1722)) => REASON_SPOOLER,
            (PrintStep::StartDoc, _) => REASON_START_DOC,
            (PrintStep::StartPage | PrintStep::EndPage, _) => REASON_PAGE,
            (PrintStep::SendImage, _) => REASON_SEND_IMAGE,
            (PrintStep::EndDoc, _) => REASON_END_DOC,
        },
    };
    reason.to_owned()
}

/// `template` with `{key}` replaced by `value`.
pub fn fill(template: &str, key: &str, value: &str) -> String {
    template.replace(&format!("{{{key}}}"), value)
}
