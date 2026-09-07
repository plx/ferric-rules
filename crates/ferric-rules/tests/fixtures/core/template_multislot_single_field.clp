; A single-field variable in a multislot requires exactly one value.
;; Level: boundary
;; Covers: patterns, template-multislot-single-field
; Protocol: load, reset, run to quiescence.
(deftemplate item (multislot tags))
(deffacts input (item (tags a)) (item (tags a b)))
(defrule observe
  (item (tags ?value))
  => (printout t (symbolp ?value) " " ?value crlf))
