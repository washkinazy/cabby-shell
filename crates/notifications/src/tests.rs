use super::*;

fn request() -> Request {
    Request {
        summary: "Synthetic notice".into(),
        ..Request::default()
    }
}
fn closed(id: u32, reason: CloseReason) -> Event {
    Event::Closed { id, reason }
}

#[test]
fn presentation_starts_expiry_and_duplicate_ack_does_not_extend_it() {
    let mut store = Store::default();
    let token = store.notify(request()).unwrap();
    let now = Instant::now();
    assert!(store.next_deadline().is_none());
    assert!(
        store
            .expire(token, now + Duration::from_secs(100))
            .is_none()
    );
    store.presented(token, now).unwrap();
    store
        .presented(token, now + Duration::from_secs(4))
        .unwrap();
    assert_eq!(
        store.next_deadline(),
        Some((token, now + Duration::from_secs(5)))
    );
    assert!(
        store
            .expire(token, now + Duration::from_millis(4999))
            .is_none()
    );
    assert_eq!(
        store.expire(token, now + Duration::from_secs(5)),
        Some(closed(token.id(), CloseReason::Expired))
    );
    assert!(store.expire(token, now + Duration::from_secs(6)).is_none());
    assert_eq!(store.close(token.id()), Err(Error::UnknownId));
    assert!(store.entries().is_empty());
}

#[test]
fn critical_and_zero_timeout_never_expire_but_explicit_duration_does() {
    let now = Instant::now();
    for (urgency, timeout, expires) in [
        (Urgency::Critical, -1, false),
        (Urgency::Critical, 1, false),
        (Urgency::Normal, 0, false),
        (Urgency::Low, 1, true),
        (Urgency::Normal, i32::MAX, true),
    ] {
        let mut store = Store::default();
        let token = store
            .notify(Request {
                urgency,
                expire_timeout: timeout,
                ..request()
            })
            .unwrap();
        store.presented(token, now).unwrap();
        assert_eq!(store.next_deadline().is_some(), expires);
        assert_eq!(
            store
                .expire(token, now + Duration::from_millis(i32::MAX as u64))
                .is_some(),
            expires
        );
    }
}

#[test]
fn queued_cards_wait_for_a_slot_and_presentation() {
    let now = Instant::now();
    let mut store = Store::default();
    let tokens: Vec<_> = (0..4).map(|_| store.notify(request()).unwrap()).collect();
    assert_eq!(store.visible().len(), 3);
    assert_eq!(store.presented(tokens[3], now), Err(Error::NotVisible));
    store.dismiss(tokens[0]).unwrap();
    assert!(store.next_deadline().is_none());
    assert_eq!(store.visible()[2].token(), tokens[3]);
    store.presented(tokens[3], now).unwrap();
    assert_eq!(store.next_deadline().unwrap().0, tokens[3]);
}

#[test]
fn replacement_invalidates_old_timers_actions_and_dismissals() {
    let now = Instant::now();
    let mut store = Store::default();
    let original = store
        .notify(Request {
            actions: vec!["open".into(), "Open".into()],
            ..request()
        })
        .unwrap();
    store.presented(original, now).unwrap();
    let replacement = store
        .notify(Request {
            replaces_id: original.id(),
            actions: vec!["open".into(), "Open again".into()],
            ..request()
        })
        .unwrap();
    assert_eq!(original.id(), replacement.id());
    assert_ne!(original, replacement);
    assert_eq!(store.entries().len(), 1);
    assert!(
        store
            .expire(original, now + Duration::from_secs(60))
            .is_none()
    );
    assert_eq!(store.dismiss(original), Err(Error::StaleToken));
    assert_eq!(store.invoke(original, "open"), Err(Error::StaleToken));
    assert_eq!(store.presented(original, now), Err(Error::StaleToken));
    assert!(store.next_deadline().is_none());
    store
        .presented(replacement, now + Duration::from_secs(10))
        .unwrap();
    assert_eq!(
        store.next_deadline().unwrap().1,
        now + Duration::from_secs(15)
    );
    assert_eq!(
        store.invoke(replacement, "old-key"),
        Err(Error::UnknownAction)
    );
    assert_eq!(
        store.close(original.id()),
        Ok(closed(original.id(), CloseReason::Client))
    );
}

#[test]
fn capacity_rejects_new_entries_but_preserves_replacement_position() {
    let mut store = Store::default();
    let tokens: Vec<_> = (0..MAX_ENTRIES)
        .map(|_| store.notify(request()).unwrap())
        .collect();
    assert_eq!(store.notify(request()), Err(Error::Capacity));
    assert_eq!(
        store.notify(Request {
            replaces_id: 500,
            ..request()
        }),
        Err(Error::Capacity)
    );
    let token = store
        .notify(Request {
            replaces_id: tokens[10].id(),
            ..request()
        })
        .unwrap();
    assert_eq!(store.entries()[10].token(), token);
    assert_eq!(store.entries().len(), MAX_ENTRIES);
    store.close(tokens[0].id()).unwrap();
    assert!(store.notify(request()).is_ok());
}

#[test]
fn malformed_replacements_leave_content_and_deadlines_unchanged() {
    let mut store = Store::default();
    let token = store.notify(request()).unwrap();
    store.presented(token, Instant::now()).unwrap();
    let deadline = store.next_deadline();
    let cases = [
        Request {
            summary: "é".repeat(129),
            ..request()
        },
        Request {
            app_name: "a".repeat(257),
            ..request()
        },
        Request {
            body: "a".repeat(MAX_BODY_BYTES + 1),
            ..request()
        },
        Request {
            actions: vec!["key".into()],
            ..request()
        },
        Request {
            actions: vec!["".into(), "label".into()],
            ..request()
        },
        Request {
            actions: vec!["a\0b".into(), "label".into()],
            ..request()
        },
        Request {
            actions: vec!["key".into(), "a".repeat(257)],
            ..request()
        },
        Request {
            actions: vec!["a".repeat(257), "label".into()],
            ..request()
        },
        Request {
            actions: vec!["key".into(), "one".into(), "key".into(), "two".into()],
            ..request()
        },
        Request {
            actions: (0..9)
                .flat_map(|i| [i.to_string(), "label".into()])
                .collect(),
            ..request()
        },
        Request {
            expire_timeout: -2,
            ..request()
        },
    ];
    for mut invalid in cases {
        invalid.replaces_id = token.id();
        assert!(store.notify(invalid).is_err());
        assert_eq!(store.next_deadline(), deadline);
        assert_eq!(store.entries()[0].token(), token);
        assert_eq!(store.entries()[0].summary, "Synthetic notice");
    }
}

#[test]
fn exact_byte_and_action_limits_are_accepted() {
    let mut store = Store::default();
    let token = store
        .notify(Request {
            app_name: "é".repeat(128),
            summary: "x".repeat(256),
            body: "x".repeat(MAX_BODY_BYTES),
            actions: (0..8)
                .flat_map(|i| [i.to_string(), "x".repeat(256)])
                .collect(),
            ..request()
        })
        .unwrap();
    assert_ne!(token.id(), 0);
    assert_eq!(store.entries()[0].actions.len(), 8);
}

#[test]
fn action_emits_before_dismissal_and_resident_action_cancels_expiry() {
    let now = Instant::now();
    for resident in [false, true] {
        let mut store = Store::default();
        let token = store
            .notify(Request {
                resident,
                actions: vec!["default".into(), "Open".into()],
                ..request()
            })
            .unwrap();
        assert_eq!(store.invoke(token, "default"), Err(Error::NotVisible));
        store.presented(token, now).unwrap();
        let events = store.invoke(token, "default").unwrap();
        assert_eq!(
            events[0],
            Event::ActionInvoked {
                id: token.id(),
                key: "default".into()
            }
        );
        if resident {
            assert_eq!(events.len(), 1);
            assert_eq!(store.entries().len(), 1);
            assert!(store.expire(token, now + Duration::from_secs(10)).is_none());
            assert_eq!(
                store.dismiss(token),
                Ok(closed(token.id(), CloseReason::Dismissed))
            );
        } else {
            assert_eq!(
                events,
                vec![
                    Event::ActionInvoked {
                        id: token.id(),
                        key: "default".into()
                    },
                    closed(token.id(), CloseReason::Dismissed)
                ]
            );
            assert_eq!(store.invoke(token, "default"), Err(Error::StaleToken));
        }
        assert!(store.entries().is_empty());
    }
}

#[test]
fn no_implicit_default_action_and_close_races_emit_once() {
    let mut store = Store::default();
    let token = store.notify(request()).unwrap();
    let now = Instant::now();
    store.presented(token, now).unwrap();
    assert_eq!(store.invoke(token, "default"), Err(Error::UnknownAction));
    assert_eq!(
        store.dismiss(token),
        Ok(closed(token.id(), CloseReason::Dismissed))
    );
    assert_eq!(store.close(token.id()), Err(Error::UnknownId));
    assert!(store.expire(token, now + Duration::from_secs(10)).is_none());
}

#[test]
fn id_allocation_skips_active_ids_and_wraps_without_zero() {
    let mut store = Store::default();
    let supplied = store
        .notify(Request {
            replaces_id: 1,
            ..request()
        })
        .unwrap();
    assert_eq!(supplied.id(), 1);
    let allocated = store.notify(request()).unwrap();
    assert_eq!(allocated.id(), 2);
    store.close(allocated.id()).unwrap();
    // Client-provided replacements cannot reset the automatic allocation cursor.
    store
        .notify(Request {
            replaces_id: 1,
            ..request()
        })
        .unwrap();
    assert_eq!(store.notify(request()).unwrap().id(), 3);
    store.next_id = u32::MAX;
    assert_eq!(store.notify(request()).unwrap().id(), u32::MAX);
    assert_eq!(store.notify(request()).unwrap().id(), 2);
}

#[test]
fn revision_exhaustion_fails_atomically_instead_of_revalidating_old_tokens() {
    let mut store = Store::default();
    let token = store.notify(request()).unwrap();
    store.revision = u64::MAX;
    assert_eq!(
        store.notify(Request {
            replaces_id: token.id(),
            ..request()
        }),
        Err(Error::RevisionExhausted)
    );
    assert_eq!(store.entries()[0].token(), token);
}

#[test]
fn plain_text_stays_literal_and_diagnostics_redact_payloads() {
    let mut store = Store::default();
    let req = Request {
        summary: "private-summary".into(),
        body: "<b>private-body</b>\nline\t\0\u{7f}".into(),
        actions: vec!["private-key".into(), "private-label".into()],
        ..request()
    };
    assert!(!format!("{req:?}").contains("private"));
    let token = store.notify(req).unwrap();
    let entry = &store.entries()[0];
    assert_eq!(entry.body, "<b>private-body</b>\nline   ");
    assert_eq!(entry.actions[0].key, "private-key");
    assert!(!format!("{entry:?} {:?}", entry.actions).contains("private"));
    store.presented(token, Instant::now()).unwrap();
    let events = store.invoke(token, "private-key").unwrap();
    assert!(!format!("{events:?}").contains("private"));
}

#[test]
fn next_timer_tracks_earliest_deadline_and_drains_equal_deadlines() {
    let mut store = Store::default();
    let now = Instant::now();
    let slow = store.notify(request()).unwrap();
    let fast = store
        .notify(Request {
            expire_timeout: 10,
            ..request()
        })
        .unwrap();
    let same = store
        .notify(Request {
            expire_timeout: 10,
            ..request()
        })
        .unwrap();
    for token in [slow, fast, same] {
        store.presented(token, now).unwrap();
    }
    assert_eq!(
        store.next_deadline(),
        Some((fast, now + Duration::from_millis(10)))
    );
    for expected in [fast, same] {
        let (token, deadline) = store.next_deadline().unwrap();
        assert_eq!(token, expected);
        assert_eq!(
            store.expire(token, deadline),
            Some(closed(token.id(), CloseReason::Expired))
        );
    }
    assert_eq!(
        store.next_deadline(),
        Some((slow, now + Duration::from_secs(5)))
    );
}

#[test]
fn supplied_id_after_closure_cannot_revive_an_old_callback() {
    let mut store = Store::default();
    let old = store
        .notify(Request {
            replaces_id: 900,
            ..request()
        })
        .unwrap();
    store.close(old.id()).unwrap();
    let new = store
        .notify(Request {
            replaces_id: 900,
            ..request()
        })
        .unwrap();
    assert_eq!(new.id(), 900);
    assert_ne!(new, old);
    assert_eq!(store.dismiss(old), Err(Error::StaleToken));
    assert_eq!(store.entries()[0].token(), new);
}
