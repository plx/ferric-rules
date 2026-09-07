; An unconstrained slot defaults to nil and a multislot to an empty multifield.
;; Level: basic
;; Covers: facts, template-implicit-defaults
; Protocol: load, reset, run to quiescence.
(deftemplate item (slot code) (multislot tags))
(deffacts input (item))
(defrule observe
  (item (code ?code) (tags $?tags))
  => (printout t ?code " " (length$ ?tags) crlf))
