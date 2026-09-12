; Identical ordered facts are deduplicated during reset.
;; Level: boundary
;; Covers: facts, duplicate-ordered-suppressed
; Protocol: load, reset, run to quiescence.
(deffacts input (value same) (value same))
(defrule observe (value ?value) => (printout t ?value crlf))
