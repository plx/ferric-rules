(deffunction walk ()
  (bind ?i 0)
  (while TRUE do
    (bind ?i (+ ?i 1))
    (if (> ?i 2) then (break))
    (printout t ?i crlf))
  after)
(defrule run => (printout t (walk) crlf))
