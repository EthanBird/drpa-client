import { save } from "@tauri-apps/plugin-dialog";
import { useCallback } from "react";

import { bytesToBase64, type OpticalReceivedFile } from "../features/optical/opticalProtocol";
import { desktopGateway } from "../infra/gateway";
import { OpticalTransferPage, type OpticalFileSaver } from "./OpticalTransferPage";

const OPTICAL_WEB_ENTRY_URL = "https://ethanbird.github.io/drpa-client/";

export function OpticalTransferDesktopPage() {
  const saveFile = useCallback<OpticalFileSaver>(async (file: OpticalReceivedFile) => {
    const target = await save({ title: "保存光学传输文件", defaultPath: file.name });
    if (!target) return null;
    return desktopGateway.saveOpticalReceivedFile(target, bytesToBase64(file.bytes));
  }, []);

  return <OpticalTransferPage saveFile={saveFile} webEntryUrl={OPTICAL_WEB_ENTRY_URL} />;
}
