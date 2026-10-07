(defrule run =>
  (bind ?outer 7)
  (loop-for-count (?i 1 2) do
    (printout t "before:" ?i ":" ?outer crlf)
    (reset)
    (printout t "after:" ?i ":" ?outer crlf))
  (printout t "done" crlf)
  (halt))
