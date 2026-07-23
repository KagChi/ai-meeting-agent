import pino from "pino";

const isProd = process.env.NODE_ENV === "production";
const level = process.env.LOG_LEVEL || (isProd ? "info" : "debug");

/** Pretty stdout in dev/Docker; structured JSON when LOG_JSON=1 or production without pretty. */
export const log = pino({
  name: "meeting-bot",
  level,
  ...(process.env.LOG_JSON === "1" || (isProd && process.env.LOG_PRETTY !== "1")
    ? {}
    : {
        transport: {
          target: "pino-pretty",
          options: {
            colorize: true,
            translateTime: "SYS:standard",
            ignore: "pid,hostname",
            singleLine: false,
          },
        },
      }),
});

export function child(bindings: Record<string, unknown>) {
  return log.child(bindings);
}
