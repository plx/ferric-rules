(defgeneric rank)
(defmethod rank (?x) (printout t "unexpected fallback" crlf) fallback)
(defmethod rank ((?x INTEGER (/ 1 0))) specific)
(defrule run => (printout t "before " (rank 1) "unexpected after" crlf))
