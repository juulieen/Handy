import React, { useState } from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "../../bindings";

interface MicrophoneSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MicrophoneSelector: React.FC<MicrophoneSelectorProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const {
      getSetting,
      updateSetting,
      resetSetting,
      isUpdating,
      isLoading,
      audioDevices,
      refreshAudioDevices,
      refreshSettings,
    } = useSettings();

    const [isDetecting, setIsDetecting] = useState(false);
    const [detectionMessage, setDetectionMessage] = useState<string | null>(
      null,
    );

    const selectedMicrophone =
      getSetting("selected_microphone") === "default"
        ? "Default"
        : getSetting("selected_microphone") || "Default";

    const handleMicrophoneSelect = async (deviceName: string) => {
      await updateSetting("selected_microphone", deviceName);
    };

    const handleReset = async () => {
      await resetSetting("selected_microphone");
    };

    const handleAutoDetect = async () => {
      setIsDetecting(true);
      setDetectionMessage("Detecting microphones...");
      try {
        const result = await commands.autoDetectMicrophone();
        if (result.status === "ok") {
          setDetectionMessage(result.data.message);
          // Refresh the audio devices and settings after detection
          await Promise.all([refreshAudioDevices(), refreshSettings()]);
          // Clear message after 5 seconds
          setTimeout(() => setDetectionMessage(null), 5000);
        } else {
          setDetectionMessage(`Detection failed: ${result.error}`);
          setTimeout(() => setDetectionMessage(null), 5000);
        }
      } catch (error) {
        setDetectionMessage(
          `Error: ${error instanceof Error ? error.message : String(error)}`,
        );
        setTimeout(() => setDetectionMessage(null), 5000);
      } finally {
        setIsDetecting(false);
      }
    };

    const microphoneOptions = audioDevices.map((device) => ({
      value: device.name,
      label: device.name,
    }));

    return (
      <SettingContainer
        title="Microphone"
        description="Select your preferred microphone device"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex flex-col space-y-2">
          <div className="flex items-center space-x-1">
            <Dropdown
              options={microphoneOptions}
              selectedValue={selectedMicrophone}
              onSelect={handleMicrophoneSelect}
              placeholder={
                isLoading || audioDevices.length === 0
                  ? "Loading..."
                  : "Select microphone..."
              }
              disabled={
                isUpdating("selected_microphone") ||
                isLoading ||
                audioDevices.length === 0 ||
                isDetecting
              }
              onRefresh={refreshAudioDevices}
            />
            <button
              onClick={handleAutoDetect}
              disabled={isDetecting || isLoading}
              className="p-1.5 rounded-md hover:bg-gray-100 dark:hover:bg-gray-800 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              title="Auto-detect best microphone"
            >
              {isDetecting ? (
                <svg
                  className="w-4 h-4 animate-spin text-gray-500"
                  xmlns="http://www.w3.org/2000/svg"
                  fill="none"
                  viewBox="0 0 24 24"
                >
                  <circle
                    className="opacity-25"
                    cx="12"
                    cy="12"
                    r="10"
                    stroke="currentColor"
                    strokeWidth="4"
                  />
                  <path
                    className="opacity-75"
                    fill="currentColor"
                    d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                  />
                </svg>
              ) : (
                <svg
                  className="w-4 h-4 text-gray-500"
                  xmlns="http://www.w3.org/2000/svg"
                  fill="none"
                  viewBox="0 0 24 24"
                  strokeWidth={1.5}
                  stroke="currentColor"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M9.348 14.652a3.75 3.75 0 010-5.304m5.304 0a3.75 3.75 0 010 5.304m-7.425 2.121a6.75 6.75 0 010-9.546m9.546 0a6.75 6.75 0 010 9.546M5.106 18.894c-3.808-3.807-3.808-9.98 0-13.788m13.788 0c3.808 3.807 3.808 9.98 0 13.788M12 12h.008v.008H12V12zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0z"
                  />
                </svg>
              )}
            </button>
            <ResetButton
              onClick={handleReset}
              disabled={
                isUpdating("selected_microphone") || isLoading || isDetecting
              }
            />
          </div>
          {detectionMessage && (
            <p className="text-xs text-gray-500 dark:text-gray-400">
              {detectionMessage}
            </p>
          )}
        </div>
      </SettingContainer>
    );
  },
);
