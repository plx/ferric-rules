(defrule run =>
  (printout t "first:[" (assert (p 1)) "] multi:[" (assert (p 2) (p 3)) "]" crlf)
  (printout t "duplicate:[" (assert (p 1)) "] last-duplicate:[" (assert (p 4) (p 1)) "]" crlf)
  (loop-for-count (?index 1 4) do (printout t ?index ":" (fact-slot-value ?index implied) crlf)))
