; The tilde field constraint excludes one literal value.
;; Level: boundary
;; Covers: patterns, negated-literal
; Protocol: load, reset, run to quiescence.
(deffacts input (color red) (color blue))
(defrule observe (color ?color&~red) => (printout t ?color crlf))
