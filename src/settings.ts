export interface Settings {
  formatter: boolean;
  hotwords: string[];
  hotkey: string;
  meterSensitivity: number;
  showCompletion: boolean;
}

export const defaultSettings: Settings = {
  formatter: true,
  hotwords: [],
  hotkey: navigator.platform.startsWith("Mac") ? "Alt+Super+R" : "Ctrl+Alt+R",
  meterSensitivity: 100,
  showCompletion: true,
};
