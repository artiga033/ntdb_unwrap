use std::env;

use protobuf::{descriptor::field_descriptor_proto::Type, reflect::MessageDescriptor};
use protobuf_codegen::{Customize, CustomizeCallback};

fn main() {
    struct ProtoGenCallback;
    impl CustomizeCallback for ProtoGenCallback {
        fn message(&self, _message: &MessageDescriptor) -> protobuf_codegen::Customize {
            Customize::default().before("#[derive(::serde::Serialize, ::serde::Deserialize)]")
        }
        fn special_field(&self, _message: &MessageDescriptor, _field: &str) -> Customize {
            Customize::default().before(r#"#[serde(serialize_with = "crate::protos::serde::serialize_special_fields", skip_deserializing, flatten)]"#)
        }
        fn field(&self, field: &protobuf::reflect::FieldDescriptor) -> Customize {
            let mut c = Customize::default();
            let proto = field.proto();
            if proto.type_() == Type::TYPE_MESSAGE && !field.is_repeated() {
                c = c.before(
                    r#"#[serde(serialize_with = "crate::protos::serde::serialize_message_field", deserialize_with = "crate::protos::serde::deserialize_message_field")]"#,
                );
            }
            c
        }
    }
    let mut gen_ = protobuf_codegen::Codegen::new();

    gen_.protoc();
    if let Ok(protoc_path) = env::var("PROTOC") {
        gen_.protoc_path(std::path::PathBuf::from(&protoc_path).as_path());
    }

    gen_.include("src/protos")
        .inputs(["src/protos/message.proto"])
        .cargo_out_dir("protos")
        .customize_callback(ProtoGenCallback);

    gen_.run_from_script();
}
