;; Issue #323: unrestricted defmethod parameter compatibility.
;; Original clips_compat/corpus/generics/unrestricted-fallback.clp reproduction.
(defgeneric classify)

;; BEGIN METHODS
(defmethod classify (?x) any)
(defmethod classify ((?x INTEGER)) integer)
;; END METHODS

(defrule probe => (printout t (classify 7) ":" (classify blue) crlf))
