# Build an image-to-video workflow from a prompt

Helixflow is the local-first AI workflow orchestrator in `majiayu000/helixflow`.
This guide describes its agent-built node graph, not other products using the
HelixFlow name. Development is frozen; it documents the existing workflow.

## Prepare the local workbench

Follow the [source quickstart](../README.md#quickstart), using the pinned Rust and
Node versions. Start the frontend-backed server and inspect `/api/ready`, which
checks the database, storage, and selected provider. `/api/health` is only process
liveness. A healthy process with no configured real provider must still refuse
real generation.

[Operations](OPERATIONS.md) describes the loopback default, credentials, durable
storage, and authentication for any non-loopback deployment. Do not expose the
workbench as a public search landing page.

## Describe the graph before running it

In a workspace using an enabled provider, ask the Agent for a concrete task:

> Create an image-to-video workflow: generate a product image on a plain
> background, then use that generated image as the video input. Use models
> available from the selected provider and a video duration they support.
> Build the graph first and explain which input is passed between the nodes.

This is a request to construct and review a graph, not a claim that every provider
currently offers the same models or that a model call has succeeded. The Agent
applies validated version transactions; inspect the nodes, image connection,
chosen models, and current version before choosing to run.

For an existing image, ask for a graph using that uploaded image as input instead.
An image-to-video operation needs an image; a text prompt alone does not satisfy
that input. The [compiler fixtures](../examples/fixtures/gh130/README.md) record
missing-image and pinned-model-mismatch cases for developers.

## Run, inspect, and recover

1. Review the cost estimate and any pending confirmation. Unknown or
   over-threshold costs must be resolved/confirmed under the current policy;
   a waiting run is not a completed generation.
2. Follow the run dock and each step's status. Inspect generated artifacts before
   accepting the result, including whether the second step used the intended image.
3. If a step fails, read its error and the **Auto fix workflow** action in the chat
   pane. A repair makes another graph version; inspect it before a new paid run.
4. To return to a known graph, choose **Restore** / **恢复** for the earlier version
   in history. Restoring a graph does not refund provider calls or certify their outputs.

Retries are bounded by `HELIXFLOW_RUN_MAX_RETRIES` and remain cost-gated. Repeated
runs can incur provider charges; an automatic retry is not an availability guarantee.

## Choose the workflow format for the destination

This workbench uses Helixflow's own graph, versions, and provider execution. If your
output needs to be a **Dify DSL** file, [Autodify](https://github.com/majiayu000/autodify#readme)
documents that generator. If you want a broader integration-oriented workflow,
[n8n's first-workflow tutorial](https://docs.n8n.io/build-your-first-workflow)
describes its own runtime. This guide does not claim interchangeable file formats
or a measured comparison between these tools.

For support, include the commit, selected provider/model, failing step, and a
redacted error in [an issue](https://github.com/majiayu000/helixflow/issues).
Provider credentials do not belong in exported graphs, screenshots, or reports.

[Back to README](../README.md) · [Configuration](../README.md#configuration) ·
[Operations](OPERATIONS.md) · [License](../LICENSE)
