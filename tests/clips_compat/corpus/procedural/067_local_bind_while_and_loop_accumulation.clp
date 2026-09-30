(deffunction accumulate (?n)
  (bind ?sum 0)
  (bind ?i 0)
  (while (< ?i ?n) do
    (bind ?i (+ ?i 1))
    (bind ?sum (+ ?sum ?i)))
  (loop-for-count (?j 1 ?n) (bind ?sum (+ ?sum ?j)))
  (create$ ?sum ?i))
(defrule probe => (printout t (accumulate 3) crlf))
