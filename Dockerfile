FROM rust:1.69 as build

# create a new empty shell project
RUN USER=root cargo new --bin graphql-check-action
WORKDIR /graphql-check-action

# copy over your manifests
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
# Cargo needs any referenced benches too, even though we're not building them
COPY ./benches ./benches

# this build step will cache your dependencies
RUN cargo build --release
RUN rm src/*.rs

# copy your source tree
COPY ./src ./src

# build for release
RUN rm ./target/release/deps/graphql_check_action*
RUN cargo build --release

# our final base
FROM gcr.io/distroless/cc AS runtime

# copy the build artifact from the build stage
COPY --from=build /graphql-check-action/target/release/graphql-check-action .

# set the startup command to run your binary
ENTRYPOINT ["/graphql-check-action"]