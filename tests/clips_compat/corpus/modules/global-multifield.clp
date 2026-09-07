;; A global can retain a multifield value.
;; Level: basic
;; Covers: modules, global-multifield
(defglobal ?*values* = (create$ red green blue))
(defrule probe => (printout t (length$ ?*values*) ":" (nth$ 2 ?*values*) crlf))
