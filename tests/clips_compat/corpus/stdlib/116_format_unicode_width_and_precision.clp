;; #340 pinned CLIPS characterization: unicode-width-and-precision

(deffacts startup (go))
(defrule exercise (go) =>
(printout t "[" (format nil "%4s|%.2s|%.1s" "é" "é" "é") "]" crlf)
)
