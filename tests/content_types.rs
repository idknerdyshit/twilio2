use serde_json::{Value, json};
use twilio2::*;

fn carousel_card(action: ContentCarouselAction) -> ContentCarouselCard {
    ContentCarouselCard::new("card body", "https://example.invalid/card.jpg").action(action)
}

#[test]
#[allow(clippy::too_many_lines)] // One fixture makes complete union coverage auditable.
fn serializes_every_content_type_and_preserves_custom_payloads() {
    let flow_option = ContentFlowOption::new("option-1", "First option");
    let flow_page = ContentFlowPage::new("page-1")
        .component(ContentFlowComponent::ShortText {
            label: "Short".to_owned(),
            text: "short-value".to_owned(),
            required: true,
            input_type: Some(ContentFlowInputType::Email),
        })
        .component(ContentFlowComponent::LongText {
            label: "Long".to_owned(),
            text: "long-value".to_owned(),
            required: false,
        })
        .component(ContentFlowComponent::SingleSelect {
            label: "One".to_owned(),
            text: "Pick one".to_owned(),
            options: ContentFlowOptions::new([flow_option.clone()]),
        })
        .component(ContentFlowComponent::MultiSelect {
            label: "Many".to_owned(),
            text: "Pick many".to_owned(),
            options: ContentFlowOptions::new([flow_option.clone()]),
        })
        .component(ContentFlowComponent::DatePicker {
            label: "Date".to_owned(),
            min_date: "2026-01-01".to_owned(),
            max_date: "2026-12-31".to_owned(),
            unavailable_dates: "[]".to_owned(),
            name: Some("appointment".to_owned()),
        })
        .component(ContentFlowComponent::List {
            label: "List".to_owned(),
            options: ContentFlowOptions::new([flow_option]),
        })
        .component(ContentFlowComponent::TextHeading {
            text: "Heading".to_owned(),
        })
        .component(ContentFlowComponent::TextSubheading {
            text: "Subheading".to_owned(),
        })
        .component(ContentFlowComponent::TextCaption {
            text: "Caption".to_owned(),
        })
        .component(ContentFlowComponent::TextBody {
            text: "Text body".to_owned(),
        })
        .component(ContentFlowComponent::RichText {
            text_list: vec!["Rich text".to_owned()],
        })
        .component(ContentFlowComponent::Media {
            url: "https://example.invalid/flow.jpg".to_owned(),
        })
        .component(ContentFlowComponent::Footer {
            label: "Footer".to_owned(),
        });

    let custom = json!({"future": {"nested": [1, true, "secret-custom"]}});
    let types = ContentTypes::new()
        .text(ContentText::new("secret-text"))
        .media(ContentMedia::new(["https://example.invalid/media.jpg"]).body("media body"))
        .location(ContentLocation::new(42.3314, -83.0458).label("Detroit"))
        .list_picker(
            ContentListPicker::new("Choose", "Open").item(ContentListItem::new(
                "one",
                "Item one",
                "Description",
            )),
        )
        .call_to_action(
            ContentCallToAction::new("Act now")
                .action(ContentCallToActionAction::url(
                    "Website",
                    "https://example.invalid/action",
                ))
                .action(ContentCallToActionAction::phone("Call", "+13135550100"))
                .action(ContentCallToActionAction::copy_code("Copy", "ABC123"))
                .action(ContentCallToActionAction::VoiceCall {
                    title: "Voice".to_owned(),
                    phone: "+13135550101".to_owned(),
                })
                .action(ContentCallToActionAction::VoiceCallRequest {
                    title: "Request".to_owned(),
                    id: "request-1".to_owned(),
                }),
        )
        .quick_reply(
            ContentQuickReply::new("Reply").action(ContentQuickReplyAction::new("Yes").id("yes")),
        )
        .card(
            ContentCard::new()
                .title("Card")
                .body("Card body")
                .orientation(ContentCardOrientation::Horizontal)
                .media("https://example.invalid/card.jpg")
                .action(ContentCardAction::Url {
                    title: "Open".to_owned(),
                    url: "https://example.invalid/open".to_owned(),
                    webview_size: Some(ContentWebviewSize::Full),
                })
                .action(ContentCardAction::phone("Call", "+13135550102"))
                .action(ContentCardAction::quick_reply("Reply", "reply-1"))
                .action(ContentCardAction::CopyCode {
                    title: "Copy".to_owned(),
                    code: "CARD123".to_owned(),
                })
                .action(ContentCardAction::VoiceCall {
                    title: "Voice".to_owned(),
                    phone: "+13135550103".to_owned(),
                }),
        )
        .carousel(
            ContentCarousel::new("Carousel")
                .card(carousel_card(ContentCarouselAction::Url {
                    title: "Open".to_owned(),
                    url: "https://example.invalid/carousel".to_owned(),
                }))
                .card(carousel_card(ContentCarouselAction::Phone {
                    title: "Call".to_owned(),
                    phone: "+13135550104".to_owned(),
                }))
                .card(carousel_card(ContentCarouselAction::QuickReply {
                    title: "Reply".to_owned(),
                    id: "carousel-reply".to_owned(),
                })),
        )
        .catalog(
            ContentCatalog::new("Catalog").id("catalog-1").item(
                ContentCatalogItem::new()
                    .id("product-1")
                    .section_title("Featured"),
            ),
        )
        .pay(ContentPay::new(
            "payment-1",
            "Pay now",
            "Merchant",
            "items-json",
            "2026-12-01T00:00:00Z",
            "Expires soon",
            "10.00",
            "10.00",
            ContentPix::OrderDetails {
                code: "pix-code".to_owned(),
                key_type: ContentPixKeyType::Evp,
                key: "pix-key".to_owned(),
            },
        ))
        .flows(ContentFlows::new("Flow", "Start", ContentFlowType::Survey).page(flow_page))
        .schedule(ContentSchedule::new(
            "schedule-1",
            "Schedule",
            "09:00-10:00",
        ))
        .whatsapp_card(
            WhatsappCard::new("WhatsApp card")
                .footer("Footer")
                .action(ContentCardAction::quick_reply("Reply", "wa-reply")),
        )
        .whatsapp_authentication(
            WhatsappAuthentication::new(WhatsappAuthenticationAction::new("Copy code"))
                .add_security_recommendation(true)
                .code_expiration_minutes(10),
        )
        .whatsapp_flows(WhatsappFlows::new("WhatsApp flow", "Start", "flow-123"))
        .custom("vendor/future", custom.clone())
        .expect("custom payload should be accepted");

    let value = serde_json::to_value(&types).expect("all payloads should serialize");
    for name in [
        "twilio/text",
        "twilio/media",
        "twilio/location",
        "twilio/list-picker",
        "twilio/call-to-action",
        "twilio/quick-reply",
        "twilio/card",
        "twilio/carousel",
        "twilio/catalog",
        "twilio/pay",
        "twilio/flows",
        "twilio/schedule",
        "whatsapp/card",
        "whatsapp/authentication",
        "whatsapp/flows",
        "vendor/future",
    ] {
        assert!(value.get(name).is_some(), "missing {name}");
    }
    assert_eq!(value["vendor/future"], custom);
    assert_eq!(value["twilio/card"]["orientation"], "HORIZONTAL");
    assert_eq!(value["twilio/card"]["actions"][0]["webview_size"], "FULL");
    assert_eq!(
        value["whatsapp/authentication"]["actions"][0]["type"],
        "COPY_CODE"
    );

    let decoded: ContentTypes = serde_json::from_value(value).expect("map should round-trip");
    assert_eq!(decoded.raw()["vendor/future"], custom);
    assert!(decoded.get_text().expect("typed decode").is_some());
    assert!(
        decoded
            .get_whatsapp_flows()
            .expect("typed decode")
            .is_some()
    );
}

#[test]
fn serializes_all_pix_status_and_key_variants() {
    for key_type in [
        ContentPixKeyType::Cpf,
        ContentPixKeyType::Cnpj,
        ContentPixKeyType::Email,
        ContentPixKeyType::Phone,
        ContentPixKeyType::Evp,
    ] {
        let value = serde_json::to_value(ContentPix::OrderDetails {
            code: "code".to_owned(),
            key_type,
            key: "key".to_owned(),
        })
        .expect("Pix details should serialize");
        assert_eq!(value["type"], "ORDER_DETAILS");
    }
    for status in [
        ContentPixStatus::Pending,
        ContentPixStatus::Processing,
        ContentPixStatus::PartiallyShipped,
        ContentPixStatus::Shipped,
        ContentPixStatus::Completed,
        ContentPixStatus::Canceled,
    ] {
        let value = serde_json::to_value(ContentPix::OrderStatus { status })
            .expect("Pix status should serialize");
        assert_eq!(value["type"], "ORDER_STATUS");
    }
}

#[test]
fn rejects_invalid_custom_and_tagged_payload_shapes() {
    assert!(ContentTypes::new().custom(" ", json!({})).is_err());
    assert!(
        ContentTypes::new()
            .custom("twilio/text", json!({"body": "raw"}))
            .is_err()
    );
    assert!(
        ContentTypes::new()
            .text(ContentText::new("body"))
            .custom("twilio/text", json!({"body": "duplicate"}))
            .is_err()
    );

    assert!(
        serde_json::from_value::<ContentCardAction>(json!({
            "type": "URL",
            "title": "missing-url"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ContentPix>(json!({
            "type": "ORDER_STATUS",
            "status": "not-a-status"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ContentFlowComponent>(json!({
            "type": "UNKNOWN",
            "text": "bad"
        }))
        .is_err()
    );

    let incompatible: ContentTypes = serde_json::from_value(json!({
        "twilio/text": {"body": 42}
    }))
    .expect("lossless maps accept future or malformed response values");
    assert!(incompatible.get_text().is_err());
}

#[test]
fn debug_output_redacts_payload_values() {
    let secret = "do-not-leak-content-value";
    let payload = ContentText::new(secret);
    let types = ContentTypes::new()
        .text(payload.clone())
        .custom("vendor/private", json!({"secret": secret}))
        .expect("custom payload should be accepted");

    for output in [format!("{payload:?}"), format!("{types:?}")] {
        assert!(!output.contains(secret));
        assert!(output.contains("<redacted>"));
    }
}

#[test]
fn enum_wire_spellings_are_stable() {
    for (size, expected) in [
        (ContentWebviewSize::Tall, "TALL"),
        (ContentWebviewSize::Full, "FULL"),
        (ContentWebviewSize::Half, "HALF"),
        (ContentWebviewSize::None, "NONE"),
    ] {
        assert_eq!(serde_json::to_value(size).expect("serialize"), expected);
    }
    for (orientation, expected) in [
        (ContentCardOrientation::Horizontal, "HORIZONTAL"),
        (ContentCardOrientation::Vertical, "VERTICAL"),
    ] {
        assert_eq!(
            serde_json::to_value(orientation).expect("serialize"),
            expected
        );
    }
    let _: Value = serde_json::to_value(ContentFlowType::Other).expect("serialize");
}
