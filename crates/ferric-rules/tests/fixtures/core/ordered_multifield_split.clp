; Two multifield variables capture values on either side of a fixed delimiter.
;; Level: boundary
;; Covers: patterns, multifield-split
; Protocol: load, reset, run to quiescence.
(deffacts input (row a marker b c))
(defrule observe
  (row $?left marker $?right)
  => (printout t (length$ ?left) " " (length$ ?right) crlf))
