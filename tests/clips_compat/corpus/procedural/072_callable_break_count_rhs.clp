(defrule r1 =>
  (loop-for-count (?i 1 10) do
    (if (> ?i 3) then (break))
    (printout t ?i crlf))
  (printout t "after" crlf))
