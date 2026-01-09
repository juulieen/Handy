import React, { useEffect, useState } from "react";
import { type as getOsType } from "@tauri-apps/plugin-os";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { commands, type PasteMethod, type LinuxEnvironmentInfo } from "@/bindings";

interface PasteMethodProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const getPasteMethodOptions = (osType: string) => {
  const baseOptions = [
    { value: "ctrl_v", label: "Clipboard (Ctrl+V)" },
    { value: "direct", label: "Direct" },
    { value: "none", label: "None" },
  ];

  // Add Shift+Insert option for Windows and Linux only
  if (osType === "windows" || osType === "linux") {
    baseOptions.push({
      value: "shift_insert",
      label: "Clipboard (Shift+Insert)",
    });
  }

  return baseOptions;
};

const getGnomeWaylandHelpMessage = (selectedMethod: string): string | null => {
  const isAutoTypeMethod =
    selectedMethod === "ctrl_v" ||
    selectedMethod === "shift_insert" ||
    selectedMethod === "direct";

  if (!isAutoTypeMethod) {
    return null;
  }

  return `On GNOME Wayland, auto-typing doesn't work. Text will be copied to clipboard - paste manually with Ctrl+V.`;
};

const getLinuxHelpMessage = (
  selectedMethod: string,
  linuxEnv: LinuxEnvironmentInfo | null,
): string | null => {
  // If we haven't loaded the env info yet, don't show any message
  if (!linuxEnv) {
    return null;
  }

  // Show GNOME Wayland-specific warning if detected
  if (linuxEnv.is_gnome_wayland) {
    return getGnomeWaylandHelpMessage(selectedMethod);
  }

  // On other Linux setups (X11, wlroots), auto-typing should work
  return null;
};

export const PasteMethodSetting: React.FC<PasteMethodProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [osType, setOsType] = useState<string>("unknown");
    const [linuxEnv, setLinuxEnv] = useState<LinuxEnvironmentInfo | null>(null);

    useEffect(() => {
      setOsType(getOsType());

      // Fetch Linux environment info
      commands.getLinuxEnvironmentInfo().then((info) => {
        setLinuxEnv(info);
      });
    }, []);

    const selectedMethod = (getSetting("paste_method") ||
      "ctrl_v") as PasteMethod;

    const pasteMethodOptions = getPasteMethodOptions(osType);

    const helpMessage =
      osType === "linux" ? getLinuxHelpMessage(selectedMethod, linuxEnv) : null;

    return (
      <SettingContainer
        title="Paste Method"
        description="Ctrl+V: 'pastes' via ctrl+v. Direct: type text directly. Shift+Insert: pastes via shift+insert. None: skip paste; just update history/clipboard."
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      >
        <div className="flex flex-col space-y-2">
          <Dropdown
            options={pasteMethodOptions}
            selectedValue={selectedMethod}
            onSelect={(value) =>
              updateSetting("paste_method", value as PasteMethod)
            }
            disabled={isUpdating("paste_method")}
          />
          {helpMessage && (
            <p className="text-xs text-amber-600 dark:text-amber-400">
              {helpMessage}
            </p>
          )}
        </div>
      </SettingContainer>
    );
  },
);
