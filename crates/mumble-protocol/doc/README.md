# mumble-protocol - Documentation

This directory contains the design documentation for the `mumble-protocol` crate,
an asynchronous Mumble client library written in Rust.

## Contents

| Document | Description |
|----------|-------------|
| [architecture.md](architecture.md) | High-level architectural overview, module map, and design principles |
| [detailed-design.md](detailed-design.md) | In-depth design of every module, data structures, and interactions |
| [diagrams/](diagrams/) | PlantUML source files for all diagrams |

## Diagrams

All diagrams are authored in [PlantUML](https://plantuml.com/).
Render them with the PlantUML CLI, VS Code extension, or any compatible tool:

```bash
# Render all diagrams to PNG
java -jar plantuml.jar diagrams/*.puml

# Or use the PlantUML Docker image
docker run --rm -v $(pwd)/diagrams:/data plantuml/plantuml "*.puml"
```

### Diagram index

| File | Type | Description |
|------|------|-------------|
| [class_overview.puml](diagrams/class_overview.puml) | Class | Top-level module and struct relationships |
| [class_transport.puml](diagrams/class_transport.puml) | Class | Transport layer (TCP/UDP) types |
| [class_audio.puml](diagrams/class_audio.puml) | Class | Audio pipeline traits, structs, and filters |
| [class_command.puml](diagrams/class_command.puml) | Class | Command pattern hierarchy |
| [class_state.puml](diagrams/class_state.puml) | Class | Server state and message types |
| [flow_connection.puml](diagrams/flow_connection.puml) | Sequence | Connection handshake flow |
| [flow_event_loop.puml](diagrams/flow_event_loop.puml) | Sequence | Main event loop dispatch |
| [flow_audio_outbound.puml](diagrams/flow_audio_outbound.puml) | Sequence | Outbound audio pipeline flow |
| [flow_audio_inbound.puml](diagrams/flow_audio_inbound.puml) | Sequence | Inbound audio pipeline flow |
| [flow_command_execution.puml](diagrams/flow_command_execution.puml) | Sequence | Command submission and execution |
| [component_overview.puml](diagrams/component_overview.puml) | Component | System component overview |
| [activity_work_queue.puml](diagrams/activity_work_queue.puml) | Activity | Priority work queue dispatch logic |
