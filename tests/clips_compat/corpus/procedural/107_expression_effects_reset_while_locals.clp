(defrule run =>
  (bind ?n 0)
  (while (< ?n 2) do
    (bind ?n (+ ?n 1))
    (printout t "before:" ?n crlf)
    (reset)
    (printout t "after:" ?n crlf))
  (printout t "done" crlf)
  (halt))
