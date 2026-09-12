;; One defglobal construct initializes multiple values.
;; Level: basic
;; Covers: modules, global-multiple
(defglobal ?*count* = 7 ?*label* = "ready")
(defrule probe => (printout t ?*count* ":" ?*label* crlf))
