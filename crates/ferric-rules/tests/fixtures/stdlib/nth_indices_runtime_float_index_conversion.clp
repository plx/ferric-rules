;; #341 pinned nth$ behavior: runtime-float-index-conversion
(deffunction identity (?x) ?x)
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (nth$ (identity 1.0) (create$ a b c)) crlf)
(printout t (nth$ (identity 1.9) (create$ a b c)) crlf)
(printout t (nth$ (identity 2.9) (create$ a b c)) crlf)
(printout t (nth$ (identity 3.1) (create$ a b c)) crlf)
(printout t (nth$ (identity 0.9) (create$ a b c)) crlf)
(printout t (nth$ (identity -0.9) (create$ a b c)) crlf)
(printout t (nth$ (identity -1.9) (create$ a b c)) crlf)
)
