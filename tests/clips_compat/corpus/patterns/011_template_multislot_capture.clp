; A template multislot supports fixed fields and a multifield suffix.
;; Level: boundary
;; Covers: patterns, template-multislot-capture
; Protocol: load, reset, run to quiescence.
(deftemplate item (multislot tags))
(deffacts input (item (tags head a b)))
(defrule observe
  (item (tags head $?values))
  => (printout t (length$ ?values) " " (nth$ 2 ?values) crlf))
