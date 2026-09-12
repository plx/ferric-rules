;; #340 pinned CLIPS characterization: integer-boundaries

(deffacts startup (go))
(defrule exercise (go) =>
(printout t "[" (format nil "%022d|%022d|%04d|%04d" 9223372036854775807 -9223372036854775808 7.9 -7.9) "]" crlf)
)
