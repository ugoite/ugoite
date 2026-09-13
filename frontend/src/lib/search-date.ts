const LOCAL_DATE_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;
const LOCAL_DATETIME_PATTERN =
  /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2})(\.\d+)?)?$/;

export type LocalDateBoundary = "start" | "end";

type LocalDateParts = {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  second: number;
  millisecond: number;
  dateOnly: boolean;
};

const invalidLocalInput = (value: string): Error =>
  new Error(`Invalid local date/time input: ${value}`);

const parseLocalInput = (value: string): LocalDateParts => {
  const dateMatch = LOCAL_DATE_PATTERN.exec(value);
  if (dateMatch) {
    return {
      year: Number(dateMatch[1]),
      month: Number(dateMatch[2]),
      day: Number(dateMatch[3]),
      hour: 0,
      minute: 0,
      second: 0,
      millisecond: 0,
      dateOnly: true,
    };
  }

  const datetimeMatch = LOCAL_DATETIME_PATTERN.exec(value);
  if (datetimeMatch) {
    const fraction = datetimeMatch[7]?.slice(1) ?? "";
    return {
      year: Number(datetimeMatch[1]),
      month: Number(datetimeMatch[2]),
      day: Number(datetimeMatch[3]),
      hour: Number(datetimeMatch[4]),
      minute: Number(datetimeMatch[5]),
      second: Number(datetimeMatch[6] ?? "0"),
      millisecond: Number(fraction.slice(0, 3).padEnd(3, "0") || "0"),
      dateOnly: false,
    };
  }

  throw invalidLocalInput(value);
};

const localDate = (parts: LocalDateParts, dayOffset = 0): Date => {
  const date = new Date(0);
  date.setHours(0, 0, 0, 0);
  date.setFullYear(parts.year, parts.month - 1, parts.day);

  if (
    date.getFullYear() !== parts.year ||
    date.getMonth() !== parts.month - 1 ||
    date.getDate() !== parts.day
  ) {
    throw invalidLocalInput(
      `${String(parts.year).padStart(4, "0")}-${
        String(parts.month).padStart(2, "0")
      }-${String(parts.day).padStart(2, "0")}`,
    );
  }

  date.setDate(date.getDate() + dayOffset);
  date.setHours(
    parts.hour,
    parts.minute,
    parts.second,
    parts.millisecond,
  );

  const expectedDay = new Date(0);
  expectedDay.setHours(0, 0, 0, 0);
  expectedDay.setFullYear(parts.year, parts.month - 1, parts.day);
  expectedDay.setDate(expectedDay.getDate() + dayOffset);

  if (
    date.getFullYear() !== expectedDay.getFullYear() ||
    date.getMonth() !== expectedDay.getMonth() ||
    date.getDate() !== expectedDay.getDate() ||
    (!parts.dateOnly && (
      date.getHours() !== parts.hour ||
      date.getMinutes() !== parts.minute ||
      date.getSeconds() !== parts.second ||
      date.getMilliseconds() !== parts.millisecond
    ))
  ) {
    throw invalidLocalInput("local date/time");
  }

  return date;
};

/**
 * Convert a browser `date` or `datetime-local` value to an explicit instant.
 * Date-only `end` values are the exclusive start of the following local
 * calendar day, so date ranges keep their calendar-day meaning across DST.
 */
export const localInputToRfc3339Instant = (
  value: string,
  boundary: LocalDateBoundary = "start",
): string => {
  const parts = parseLocalInput(value);
  if (boundary === "end" && !parts.dateOnly) {
    throw invalidLocalInput(value);
  }
  return localDate(parts, boundary === "end" ? 1 : 0).toISOString();
};
