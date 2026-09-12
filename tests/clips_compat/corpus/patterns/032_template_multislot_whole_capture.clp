; A sole multifield variable captures every explicit multislot value.
;; Level: basic
;; Covers: patterns, template-multislot-whole-capture
; Protocol: load, reset, run to quiescence.
(deftemplate item (multislot tags))
(deffacts input (item (tags a b)))
(defrule observe
  (item (tags $?values))
  => (printout t (length$ ?values) " " (nth$ 1 ?values) " " (nth$ 2 ?values) crlf))
