;; Issue #323: unrestricted defmethod parameter compatibility.
(defgeneric zero)

;; BEGIN METHODS
(defmethod zero () zero)
;; END METHODS

(defrule probe => (printout t (zero) crlf))
