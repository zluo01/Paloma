# DeepSeek Provider

A [Paloma](../../..) provider plugin serving DeepSeek models through the
official API. Advertises provider `DeepSeek` with a single backend,
`DeepSeek API` (API-key auth); lives in `plugins/providers/deepseek`.

## Installation

### Package

Requires JDK 25 and Maven.

```sh
mvn package
```

The build produces `target/deepseek-provider-fat.jar` — a single
self-contained jar with every dependency shaded in; copy it anywhere stable.

### Connect

Register the jar under **Settings → Plugins → Add Provider Plugin…** with
**Command** `java` and **Arguments**
`["-jar", "/absolute/path/to/deepseek-provider-fat.jar"]`.

## Models

The catalogue is static — `src/main/resources/models.json`. Whenever there is a new publication on new models, need to update the file.

## Development

```sh
mvn verify           # checkstyle + google-java-format gate, tests, fat jar
mvn test             # JUnit 5 unit tests (the section state machines)
mvn spotless:apply   # rewrite formatting in place
```
