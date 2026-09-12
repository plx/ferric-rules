; A predicate constraint can compare against an earlier pattern binding.
;; Level: interaction
;; Covers: patterns, predicate-join
; Protocol: load, reset, run to quiescence.
(deffacts input (threshold 2) (value 2) (value 3))
(defrule observe
  (threshold ?limit) (value ?value&:(> ?value ?limit))
  => (printout t ?value crlf))
