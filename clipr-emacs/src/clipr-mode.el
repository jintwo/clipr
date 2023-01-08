;;; clipr-mode.el --- Clipr major mode
;;; Commentary:
;;; Code:

(require 'dash)
(require 'custom)
(require 'hl-line)

(require 'clipr)

(defcustom clipr-buffer-name "Clipr"
  "Clipr buffer name."
  :type 'string
  :group 'clipr)

(defcustom clipr-config-path "~/config/clipr.toml"
  "Clipr config path."
  :type 'string
  :group 'clipr)

(defconst clipr-list-format
  [("Pos" 7 t)
   ("Content" 35 t)]
  "Clipr list format.")

(defun clipr-create ()
  "Create Clipr."
  (let* ((clipr-buffer (get-buffer-create clipr-buffer-name))
         (buffer-window (get-buffer-window clipr-buffer)))
    (if buffer-window
        (display-buffer clipr-buffer)
      (let ((new-window (split-window-below)))
        (set-window-buffer new-window clipr-buffer)
        (with-current-buffer clipr-buffer
          (funcall 'clipr-mode))))))

(defun clipr-kill ()
  "Kill Clipr."
  (interactive)
  (with-current-buffer clipr-buffer-name
    (kill-buffer-and-window)))

(defun clipr-refresh ()
  "Refresh Clipr."
  (interactive)
  (with-current-buffer clipr-buffer-name
    (tabulated-list-print :remember-pos)
    (hl-line-highlight)))

(defun clipr-list-entries ()
  "Get entries for Clipr."
  (-map
   (lambda (entry)
     (let ((pos (plist-get entry :pos))
           (content (plist-get entry :content)))
       (list pos (vector (number-to-string pos) content))))
   (clipr-cmd "list")))

(defvar clipr-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map (kbd "d") 'clipr-delete)
    (define-key map (kbd "g") 'clipr-refresh)
    (define-key map (kbd "C") 'clipr-select)
    (define-key map (kbd "RET") 'clipr-select-and-quit)
    (define-key map (kbd "q") 'clipr-kill)
    map)
  "Keymap for Clipr.")

(defun clipr-delete ()
  "Delete selected entry."
  (interactive)
  (clipr-cmd (format "del %d" (tabulated-list-get-id)))
  (clipr-refresh))

(defun clipr-select ()
  "Copy selected entry to clipboard."
  (interactive)
  (clipr-cmd (format "set %d" (tabulated-list-get-id)))
  (pulse-momentary-highlight-one-line))

(defun clipr-select-and-quit ()
  "Copy selected entry to clipboard."
  (interactive)
  (clipr-select)
  (clipr-kill))

(define-derived-mode clipr-mode tabulated-list-mode "Clipr"
  (buffer-disable-undo)
  (kill-all-local-variables)
  (setq truncate-lines t)
  (setq mode-name "Clipr")
  (setq major-mode 'clipr-mode)
  (use-local-map clipr-mode-map)
  (setq tabulated-list-format clipr-list-format)
  (setq tabulated-list-entries 'clipr-list-entries)
  (tabulated-list-init-header)
  (tabulated-list-print)
  (hl-line-mode 1))

(defun clipr-show ()
  "Show Clipr."
  (interactive)
  (clipr-create))

(provide 'clipr-mode)
;;; clipr-mode.el ends here
