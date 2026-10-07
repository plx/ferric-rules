(defrule run =>
  (loop-for-count 2 do (printout t "literal" crlf))
  (bind ?n 2)
  (loop-for-count ?n do (printout t "variable" crlf))
  (loop-for-count (+ 1 1) do (printout t "expression" crlf))
  (loop-for-count (?i 2) do (printout t "named:" ?i crlf))
  (loop-for-count (?i 2 3) do (printout t "range:" ?i crlf)))
