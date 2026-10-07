(defgeneric walk)
(defmethod walk ((?n INTEGER))
  (loop-for-count (?i 1 ?n) do
    (if (> ?i 2) then (break))
    (printout t ?i crlf))
  after)
(defrule run => (printout t (walk 5) crlf))
