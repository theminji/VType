import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./AnalyticsPanel.css";

interface Analytics {
  totals: {
    dictations: number;
    spokenWords: number;
    fillerWords: number;
    insertedWords: number;
    recordingMs: number;
    processingMs: number;
  };
  activity: { date: string; spokenWords: number }[];
  typingSpeedWpm: number;
}

const number = new Intl.NumberFormat();
const weekday = new Intl.DateTimeFormat(undefined, { weekday: "short", timeZone: "UTC" });
const fullDate = new Intl.DateTimeFormat(undefined, {
  weekday: "long", month: "short", day: "numeric", timeZone: "UTC",
});

function duration(milliseconds: number) {
  const seconds = Math.round(Math.abs(milliseconds) / 1000);
  if (seconds < 60) return `${seconds} sec`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${number.format(minutes)} min`;
  const remainder = minutes % 60;
  return `${number.format(Math.floor(minutes / 60))} hr${remainder ? ` ${remainder} min` : ""}`;
}

export default function AnalyticsPanel() {
  const [data, setData] = useState<Analytics>();
  const [failed, setFailed] = useState(false);
  const [retry, setRetry] = useState(0);

  useEffect(() => {
    let active = true;
    let latest = 0;
    async function refresh() {
      const request = ++latest;
      try {
        const next = await invoke<Analytics>("get_analytics");
        if (active && request === latest) {
          setData(next);
          setFailed(false);
        }
      } catch {
        if (active && request === latest) setFailed(true);
      }
    }
    const listeners = ["analytics-changed", "settings-changed", "panel-opened"].map(
      (event) => listen(event, refresh),
    );
    void Promise.allSettled(listeners).then(() => { if (active) void refresh(); });
    return () => {
      active = false;
      for (const listener of listeners) void listener.then((stop) => stop(), () => {});
    };
  }, [retry]);

  if (failed) return (
    <div className="analytics analytics-message">
      <p role="status">Analytics couldn’t be loaded.</p>
      <button type="button" onClick={() => setRetry((value) => value + 1)}>Try again</button>
    </div>
  );
  if (!data) return <p className="analytics analytics-message" role="status">Loading analytics…</p>;

  const { totals, activity, typingSpeedWpm } = data;
  if (!totals.dictations) return (
    <div className="analytics analytics-empty">
      <div className="analytics-empty-mark" aria-hidden="true"><i /><i /><i /><i /><i /></div>
      <h1>No dictations yet</h1>
      <p>Use your shortcut to dictate into any text field. Your word count, speaking pace, and estimated time saved will appear here after your first successful insertion.</p>
    </div>
  );

  const difference = totals.insertedWords * 60_000 / typingSpeedWpm
    - totals.recordingMs - totals.processingMs;
  const pace = totals.recordingMs
    ? number.format(Math.round(totals.spokenWords * 60_000 / totals.recordingMs))
    : "—";
  const highest = Math.max(1, ...activity.map((day) => day.spokenWords));
  const weekTotal = activity.reduce((sum, day) => sum + day.spokenWords, 0);

  return (
    <div className="analytics">
      <header className="analytics-hero">
        <div className="analytics-hero-top">
          <h1>{difference < 0 ? "Estimated extra time" : "Estimated time saved"}</h1>
          <span>All time</span>
        </div>
        <p className="analytics-time">{duration(difference)}</p>
        <p className="analytics-baseline">Compared with typing at {typingSpeedWpm} words per minute</p>
      </header>

      <dl className="analytics-counts">
        <div><dt>Words spoken</dt><dd>{number.format(totals.spokenWords)}</dd></div>
        <div><dt>Fillers detected</dt><dd>{number.format(totals.fillerWords)}</dd></div>
      </dl>

      <section className="analytics-activity" aria-labelledby="dictation-activity">
        <div className="analytics-activity-heading">
          <h2 id="dictation-activity">Words spoken by day</h2>
          <span>Last 7 days</span>
        </div>
        {weekTotal ? <ul className="analytics-chart">
          {activity.map(({ date, spokenWords }) => {
            const day = new Date(`${date}T00:00:00Z`);
            const label = `${fullDate.format(day)}: ${number.format(spokenWords)} ${spokenWords === 1 ? "word" : "words"}`;
            return (
              <li key={date} title={label}>
                <span className="analytics-sr-only">{label}</span>
                <div className="analytics-chart-track" aria-hidden="true">
                  <span className="analytics-chart-bar" style={{ height: `${spokenWords / highest * 100}%` }} />
                </div>
                <span aria-hidden="true">{weekday.format(day)}</span>
              </li>
            );
          })}
        </ul> : <p className="analytics-inactive">No dictations in the last 7 days.</p>}
      </section>

      <dl className="analytics-secondary">
        <div><dt>Dictations</dt><dd>{number.format(totals.dictations)}</dd></div>
        <div><dt>Words inserted</dt><dd>{number.format(totals.insertedWords)}</dd></div>
        <div><dt>Speaking pace</dt><dd>{pace}{pace !== "—" && <small> wpm</small>}</dd></div>
      </dl>

      <details className="analytics-method">
        <summary>How these numbers are calculated</summary>
        <p>VType counts recognized words and common English fillers like “um” and “uh” before formatting. Only successful insertions count. Counts and timing stay on this device.</p>
        <p>Estimated time compares how long it would take to type the inserted words with the time spent recording, processing, and inserting them. Change your typing speed in Settings.</p>
      </details>
    </div>
  );
}
