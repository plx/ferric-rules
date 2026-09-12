; A test CE filters a correlated pair after variables are bound.
;; Level: interaction
;; Covers: patterns, test-ce-join
; Protocol: load, reset, run to quiescence.
(deffacts input (left 2) (right 1) (right 3))
(defrule observe
  (left ?left) (right ?right) (test (> ?right ?left))
  => (printout t ?left " " ?right crlf))
