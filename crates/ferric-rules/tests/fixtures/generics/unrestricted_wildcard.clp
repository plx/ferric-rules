;; Issue #323: unrestricted defmethod parameter compatibility.
(defgeneric countargs)

;; BEGIN METHODS
(defmethod countargs ($?rest) (length$ ?rest))
;; END METHODS

(defrule probe => (printout t (countargs) ":" (countargs a 2 "three") crlf))
