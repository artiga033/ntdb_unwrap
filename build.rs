fn main(){
    protobuf_codegen::Codegen::new()
    .protoc()
    .include("src/protos")
    .inputs(["src/protos/message.proto"])
    .cargo_out_dir("protos")
    .run_from_script();
}