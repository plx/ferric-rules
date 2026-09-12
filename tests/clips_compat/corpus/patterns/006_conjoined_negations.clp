; Conjoined constraints must all hold for the same field.
;; Level: boundary
;; Covers: patterns, conjoined-negations
; Protocol: load, reset, run to quiescence.
(deffacts input (color red) (color blue) (color green))
(defrule observe
  (color ?color&~red&~blue) => (printout t ?color crlf))
