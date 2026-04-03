import { TextAttributes } from "@opentui/core"
import { useKeyboard } from "@opentui/solid"
import { useTheme } from "@tui/context/theme"
import { useDialog } from "@tui/ui/dialog"

interface DialogConfirmProps {
  title: string
  message: string
  confirmLabel?: string
  onConfirm: () => void
}

export function DialogConfirm(props: DialogConfirmProps) {
  const dialog = useDialog()
  const { theme } = useTheme()

  useKeyboard((evt) => {
    if (evt.name === "return" || evt.name === "y") {
      evt.preventDefault()
      dialog.pop()
      props.onConfirm()
    }
    if (evt.name === "escape" || evt.name === "n") {
      evt.preventDefault()
      dialog.pop()
    }
  })

  return (
    <box gap={1} paddingBottom={1}>
      <box paddingLeft={4} paddingRight={4}>
        <text fg={theme.warning} attributes={TextAttributes.BOLD}>
          {props.title}
        </text>
      </box>
      <box paddingLeft={4} paddingRight={4}>
        <text fg={theme.text} wrapMode="word">
          {props.message}
        </text>
      </box>
      <box paddingLeft={4} paddingRight={4} flexDirection="row" gap={2}>
        <box
          backgroundColor={theme.error}
          padding={1}
          onMouseUp={() => { dialog.pop(); props.onConfirm() }}
        >
          <text fg={theme.selectedListItemText} attributes={TextAttributes.BOLD}>
            {props.confirmLabel || "Delete"} (y)
          </text>
        </box>
        <box
          backgroundColor={theme.backgroundElement}
          padding={1}
          onMouseUp={() => dialog.pop()}
        >
          <text fg={theme.text}>
            Cancel (n/esc)
          </text>
        </box>
      </box>
    </box>
  )
}
