(defgeneric size)
(defmethod size ($?r) (str-cat "short:" (length$ ?r)))
(defmethod size (($?r (>= (length$ ?r) 3))) (str-cat "long:" (length$ ?r)))
(defrule run =>
  (printout t (size (create$ a b c)) ":" (size x (create$ a b)) ":" (size (create$ a b)) crlf))
