;; Issue #323: unrestricted defmethod parameter compatibility.
(defgeneric countargs)

(defmethod countargs ($?rest) (length$ ?rest))

(defrule probe => (printout t (countargs) ":" (countargs a 2 "three") crlf))
