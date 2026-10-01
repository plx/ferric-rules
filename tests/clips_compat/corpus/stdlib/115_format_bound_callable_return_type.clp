;; #340 pinned CLIPS characterization: bound-callable-return-type
(deffunction padded (?x) (format nil "%04d" ?x))
(deffacts startup (go 7))
(defrule exercise (go ?n) =>
(printout t (stringp (format nil "%04d" ?n)) ":" (padded ?n) crlf)
)
