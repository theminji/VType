import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import AnalyticsPanel from "./AnalyticsPanel";
import SettingsPanel from "./SettingsPanel";

const tabs = ["Analytics", "Settings"] as const;
type Tab = typeof tabs[number];

export default function PreferencesWindow() {
  const [tab, setTab] = useState<Tab>("Analytics");
  const tabList = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const listener = listen("panel-opened", () => setTab("Analytics"));
    return () => { void listener.then((stop) => stop(), () => {}); };
  }, []);

  return (
    <main className="vtype-panel">
      <div className="panel-tabs" role="tablist" aria-label="VType" ref={tabList}
        onKeyDown={(event) => {
          let next: Tab;
          if (event.key === "Home") next = "Analytics";
          else if (event.key === "End") next = "Settings";
          else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
            next = tab === "Analytics" ? "Settings" : "Analytics";
          } else return;
          event.preventDefault();
          setTab(next);
          tabList.current?.querySelector<HTMLButtonElement>(`#tab-${next}`)?.focus();
        }}>
        {tabs.map((name) => (
          <button key={name} type="button" role="tab" id={`tab-${name}`}
            aria-controls={`panel-${name}`} aria-selected={tab === name}
            tabIndex={tab === name ? 0 : -1} onClick={() => setTab(name)}>
            {name}
          </button>
        ))}
      </div>
      <section role="tabpanel" id="panel-Analytics" aria-labelledby="tab-Analytics" tabIndex={0}
        hidden={tab !== "Analytics"}>
        <AnalyticsPanel />
      </section>
      <section role="tabpanel" id="panel-Settings" aria-labelledby="tab-Settings" tabIndex={0}
        hidden={tab !== "Settings"}>
        <SettingsPanel />
      </section>
    </main>
  );
}
