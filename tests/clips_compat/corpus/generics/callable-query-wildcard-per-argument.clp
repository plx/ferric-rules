(defgeneric z)
(defmethod z ($?r) no)
(defmethod z (($?r (> (length$ ?r) 0))) yes)
(defrule run => (printout t (z) crlf) (printout t (z a) crlf))
