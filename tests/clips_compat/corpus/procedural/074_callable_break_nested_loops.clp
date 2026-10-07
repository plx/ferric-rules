(defrule run =>
  (loop-for-count (?i 1 2) do
    (loop-for-count (?j 1 3) do
      (if (> ?j 1) then (break))
      (printout t ?i ":" ?j crlf))
    (printout t "outer:" ?i crlf))
  (printout t "after" crlf))
