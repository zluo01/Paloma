package helper;

import io.netty.handler.codec.http.HttpHeaderValues;
import io.vertx.core.Future;
import io.vertx.core.VerticleBase;
import io.vertx.core.buffer.Buffer;
import io.vertx.core.http.HttpHeaders;
import io.vertx.core.http.HttpServer;
import io.vertx.core.http.HttpServerRequest;
import io.vertx.core.json.JsonObject;
import java.io.IOException;

/**
 * Serves recorded SSE fixtures. The request body selects the behavior:
 *
 * <ul>
 *   <li>{@code {"fixture": <name>}} streams the fixture as chunked text/event-stream
 *   <li>{@code {"status": <code>, "body": <json>}} replies with that status and body
 * </ul>
 */
public final class MockSseVerticle extends VerticleBase {
  public static final String FIXTURE = "fixture";
  public static final String STATUS = "status";
  public static final String BODY = "body";

  // deliberately smaller than a fixture line so SSE chunks split lines mid-buffer
  private static final int CHUNK_SIZE = 100;

  private HttpServer server;

  @Override
  public Future<?> start() {
    server = vertx.createHttpServer();
    return server
        .requestHandler(
            request -> request.body().onSuccess(body -> handle(request, new JsonObject(body))))
        .listen(0);
  }

  public int port() {
    return server.actualPort();
  }

  private void handle(final HttpServerRequest request, final JsonObject body) {
    final var status = body.getInteger(STATUS, 200);
    if (status != 200) {
      request.response().setStatusCode(status).end(body.getJsonObject(BODY).encode());
      return;
    }

    final Buffer raw;
    try {
      raw = Fixtures.raw(body.getString(FIXTURE));
    } catch (IOException | RuntimeException e) {
      request.response().setStatusCode(500).end(String.valueOf(e.getMessage()));
      return;
    }

    final var response =
        request.response().putHeader(HttpHeaders.CONTENT_TYPE, HttpHeaderValues.TEXT_EVENT_STREAM);

    for (var offset = 0; offset < raw.length(); offset += CHUNK_SIZE) {
      response.write(raw.getBuffer(offset, Math.min(offset + CHUNK_SIZE, raw.length())));
    }
    response.end();
  }
}
