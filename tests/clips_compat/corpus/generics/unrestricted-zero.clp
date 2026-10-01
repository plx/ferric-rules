;; Issue #323: unrestricted defmethod parameter compatibility.
(defgeneric zero)

(defmethod zero () zero)

(defrule probe => (printout t (zero) crlf))
