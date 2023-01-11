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

(defcustom clipr-item-preview-length 128
  "Clipr item preview lingth."
  :type 'integer
  :group 'clipr)

(defconst clipr-list-format
  [("Pos" 7 t)
   ("Date" 13 t)
   ("Tags" 16 t)
   ("Content" 35 nil)]
  "Clipr list format.")

(defconst clipr--default-query-cmd (string-join (list "list" "0" "0" (number-to-string clipr-item-preview-length)) " "))

(defconst clipr--default-action 'clipr-select-and-quit)

(defvar clipr--query-cmd clipr--default-query-cmd)

(defun clipr-create ()
  "Create Clipr."
  (let* ((clipr-buffer (get-buffer-create clipr-buffer-name))
         (buffer-window (get-buffer-window clipr-buffer)))
    (when (not buffer-window)
        (with-current-buffer clipr-buffer
          (funcall 'clipr-mode)))
    (select-window (display-buffer clipr-buffer))))

(defun clipr-kill ()
  "Kill Clipr."
  (interactive)
  (with-current-buffer clipr-buffer-name
    (kill-buffer)))

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
           (content (plist-get entry :content))
           (tags (plist-get entry :tags))
           (date (plist-get entry :date)))
       (list pos (vector
                  (cons (number-to-string pos) `(face default action ,clipr--default-action))
                  (cons date `(face default action ,clipr--default-action))
                  (cons tags `(face bold action ,clipr--default-action))
                  (cons content `(face font-lock-comment-face action ,clipr--default-action))))))
   (clipr-cmd clipr--query-cmd)))

(defvar clipr-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map (kbd "d") 'clipr-delete)
    (define-key map (kbd "g") 'clipr-refresh)
    (define-key map (kbd "RET") 'clipr-select-and-quit)
    (define-key map (kbd "q") 'clipr-kill)
    (define-key map (kbd "+") 'clipr-tag)
    (define-key map (kbd "-") 'clipr-untag)
    (define-key map (kbd "t") 'clipr-filter-by-tag)
    (define-key map (kbd "c") 'clipr-filter-clear)
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
  (let ((cmd (format "set %d" (tabulated-list-get-id))))
    (clipr-cmd cmd)
    (pulse-momentary-highlight-one-line)))

(defun clipr-select-and-quit (arg)
  "Copy selected entry to clipboard."
  (interactive)
  (clipr-select)
  (clipr-kill))

(defun clipr--read-tag ()
  (let ((tags (string-split (car (aref (tabulated-list-get-entry) 2)) ":")))
    (list (completing-read "Tag: " tags))))

(defun clipr-tag (tag)
  "Tag selected entry."
  (interactive (clipr--read-tag))
  (clipr-cmd (format "tag %d %s" (tabulated-list-get-id) tag))
  (clipr-refresh))

(defun clipr-untag (tag)
  "Untag selected entry."
  (interactive (clipr--read-tag))
  (clipr-cmd (format "untag %d %s" (tabulated-list-get-id) tag))
  (clipr-refresh))

(defun clipr--read-all-tags ()
  (let ((tags (string-split (clipr-cmd "tags") ":")))
    (list (completing-read "Tag: " tags))))

(defun clipr-filter-by-tag (tag)
  "Filter entries by tag"
  (interactive (clipr--read-all-tags))
  (if (> (length tag) 0)
      (setq clipr--query-cmd (format "select -- tag '%s'" tag))
    (setq clipr--query-cmd clipr--default-query-cmd))
  (clipr-refresh))

(defun clipr-filter-clear ()
  "Clear filter"
  (interactive)
  (setq clipr--query-cmd clipr--default-query-cmd)
  (clipr-refresh))

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
