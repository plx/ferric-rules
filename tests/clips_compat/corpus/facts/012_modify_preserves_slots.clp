; Modify updates one slot while preserving all omitted slots.
;; Level: interaction
;; Covers: facts, modify-preserves-slots
; Protocol: load, reset, run to quiescence.
(deftemplate item (slot name) (slot count))
(deffacts input (item (name blue) (count 1)))
(defrule update
  ?f <- (item (count 1))
  => (modify ?f (count 2)))
(defrule observe
  (item (name ?name) (count 2))
  => (printout t ?name " 2" crlf))
