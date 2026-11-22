//! The phone actions of `App::do_action`: pairing a phone and taking one off the list. `use super::*` gives them what `app.rs` itself
//! uses (`parse`, `tryr!`, the model types); they are the arms of that match as they were, moved here so no one
//! function holds every action of the app.
use super::*;

/// Whether `action` is one of these.
pub(super) fn handles(action: &str) -> bool {
    matches!(action, "pair/start" | "pair/stop" | "phone/revoke")
}

/// Runs one of them. `App::do_action` sends an action here only when `handles` says so.
pub(super) async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // ---- phone remote ----
        "pair/start" => {
            let (pin, secret) = (random_pin(), random_token());
            let url = app.lan_url.clone().unwrap_or_default();
            let qr = (!url.is_empty()).then(|| pair_qr_svg(&pair_link(&url, &secret))).flatten();
            *app.pairing.lock().unwrap() = Some(Pairing { pin: pin.clone(), secret, until: Instant::now() + Duration::from_secs(120), failures: 0 });
            Ok(json!({ "pin": pin, "url": url, "qr": qr, "seconds": 120 }))
        }
        "pair/stop" => { *app.pairing.lock().unwrap() = None; Ok(json!({})) }
        "phone/revoke" => {
            let Id { id } = parse(body)?;
            let mut phones = app.phones.lock().unwrap();
            phones.retain(|p| p.id != id);
            app.save_phones(&phones);
            drop(phones);
            let _ = app.tx.send(Update::Revoked { phone: id });
            app.broadcast_data();
            Ok(json!({}))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}
