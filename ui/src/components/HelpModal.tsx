import { Modal, ModalBody, ModalContent, ModalHeader } from "@heroui/react";
import { useAtom } from "jotai";
import { helpOpenAtom } from "../state/atoms";
import { SHORTCUTS } from "../hooks/useShortcuts";

export function HelpModal() {
  const [open, setOpen] = useAtom(helpOpenAtom);
  return (
    <Modal isOpen={open} onOpenChange={setOpen} size="md" backdrop="blur">
      <ModalContent>
        <ModalHeader className="text-sm uppercase tracking-wider text-default-500">
          keyboard shortcuts
        </ModalHeader>
        <ModalBody className="pb-6">
          <div className="grid grid-cols-1 gap-1.5">
            {SHORTCUTS.map((s) => (
              <div key={s.label} className="flex items-center justify-between gap-4">
                <span className="text-sm text-default-600">{s.label}</span>
                <span className="flex gap-1">
                  {s.keys.map((k) => (
                    <kbd key={k} className="neon-key">
                      {k}
                    </kbd>
                  ))}
                </span>
              </div>
            ))}
          </div>
        </ModalBody>
      </ModalContent>
    </Modal>
  );
}
