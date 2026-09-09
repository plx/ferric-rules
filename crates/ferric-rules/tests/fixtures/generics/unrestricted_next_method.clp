;; Issue #323: unrestricted defmethod parameter compatibility.
(defgeneric describe)

;; BEGIN METHODS
(defmethod describe (?x) (str-cat "any:" ?x))
(defmethod describe ((?x INTEGER)) (str-cat "integer:" (call-next-method)))
;; END METHODS

(defrule probe => (printout t (describe 7) crlf (describe blue) crlf))
