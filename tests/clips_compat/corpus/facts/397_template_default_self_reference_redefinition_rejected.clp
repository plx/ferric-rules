;; A redefinition's default cannot assert the template it replaces: CLIPS removes
;; the old definition before parsing the new body, so the slot reads as a call.
;; Level: boundary
;; Covers: deftemplate, default-dynamic, compile-time-validation
(deftemplate item (slot original))
(deftemplate item (slot replacement (default-dynamic (assert (item (original 7))))))
