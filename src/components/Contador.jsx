import { useState, useEffect } from "preact/hooks";

const NOTITAS = [
  "Feliz Día del Cariño, espero que te guste mucho este detallito. Cada día tendrás unas frases. 🌻",
  "Eres lo máximo, estoy muy orgulloso de ti, princesa. 💜",
  "Cada día más cerca de tu cumple, cada día más cerca de que te comas a un menor. 🫣",
  "Ve aceptando que te amo y que nuestra relación funcionará, con todo y sus defectos. 🙄",
  "¿Cómo te explico lo maravillosa que eres? 👀",
  "Espero que estés teniendo un lindo día... conmigo. :)",
  "No se trata de ser perfectos, se trata de ESTAR JUNTOS. 💜",
  "¿Ya te viste al espejo? Es que estás hermosa. 😘",
  "Uy, creo que ya casi es hora de que puedas tomar tequila legalmente. 🍻",
  "No sé ni qué decirte en este punto, estoy tan orgulloso de la persona en la que te has convertido. ¡Feliz casi cumpleaños! 💜",
];

const TEXTOS = [
  "aveces...",
  "todo lo que necesitamos",
  "es estar juntos :)",
  "Felices 18...",
  "que se siente...",
  "salir con un menor? :o",
  "Sabes que te gusta JAJAJA",
  "Te amo mucho con demasiado 💋",
  "Y creo que tengo un regalo",
  "Ten, ten, ten 💜",
];

const ULTIMOTEXTO = "Aaaa pero se me olvido esto tambien";

function calcularTiempoRestante(fechaEvento) {
  const ahora = new Date();
  const diferencia = fechaEvento.getTime() - ahora.getTime();

  if (diferencia <= 0) {
    return {
      dias: 0,
      horas: 0,
      minutos: 0,
      segundos: 0,
      formato: "00d 00h 00m 00s",
    };
  }

  const dias = Math.floor(diferencia / (1000 * 60 * 60 * 24));
  const horas = Math.floor(
    (diferencia % (1000 * 60 * 60 * 24)) / (1000 * 60 * 60)
  );
  const minutos = Math.floor((diferencia % (1000 * 60 * 60)) / (1000 * 60));
  const segundos = Math.floor((diferencia % (1000 * 60)) / 1000);

  return {
    dias,
    horas,
    minutos,
    segundos,
    formato: `${dias}d ${horas}h ${minutos}m ${segundos}s`,
  };
}

function Contador({ onSorpresa }) {
  const fechaEvento = new Date("2025-02-24T05:00:00Z");
  const [tiempoRestante, setTiempoRestante] = useState(
    calcularTiempoRestante(fechaEvento)
  );

  useEffect(() => {
    const intervalo = setInterval(() => {
      setTiempoRestante(calcularTiempoRestante(fechaEvento));
    }, 1000);
    return () => clearInterval(intervalo);
  }, []);

  if (
    tiempoRestante.dias === 0 &&
    tiempoRestante.horas === 0 &&
    tiempoRestante.minutos === 0 &&
    tiempoRestante.segundos === 0
  ) {
    return (
      <div className="w-full h-screen flex items-center justify-center flex-col text-2xl font-bold">
        <p className="absolute top-2 text-sm">
          Con 💜 para mi Mendi Mensita 💋
        </p>
        <p className="text-xl">¡FELIZ CUMPLEAÑOS PRINCESA! 💜</p>
        <button
          onClick={onSorpresa}
          className="bg-purple-500 hover:bg-purple-400 text-white py-2 px-4 rounded-full font-semibold mt-4 text-xl"
        >
          ¡Sorpresa! 👀
        </button>
        <p className="absolute bottom-8 text-sm">
          Recomiendo que te pongas audifonos :)
        </p>
      </div>
    );
  }

  const indiceNotita = 9 - Math.min(tiempoRestante.dias, 9);
  const notitaDia = NOTITAS[indiceNotita];

  return (
    <div className="w-full h-screen flex items-center justify-center flex-col text-2xl font-bold">
      <p className="absolute top-2 text-sm">Con 💜 para mi Mendi Mensita 💋</p>
      <p className="text-xl">aún no puedes verlo :) falta:</p>
      <p className="text-5xl pt-2">
        {`${tiempoRestante.dias} : ${tiempoRestante.horas} : ${tiempoRestante.minutos} : ${tiempoRestante.segundos}`}
      </p>
      <p className="absolute bottom-2 text-xs px-6 text-center text-pretty">
        {`Notita del día: ${notitaDia}`}
      </p>
    </div>
  );
}

function BtnDescargar({ link, nombre, onDownload }) {
  const handleDownload = () => {
    const a = document.createElement("a");
    a.href = link;
    a.download = link.split("/").pop();
    a.click();
    if (onDownload) onDownload();
  };

  return (
    <button
      type="button"
      onClick={handleDownload}
      className="text-white bg-purple-500 hover:bg-purple-600 focus:ring-4 focus:outline-none focus:ring-purple-300 font-medium rounded-full text-sm p-2.5 text-center inline-flex items-center me-2"
    >
      <svg
        className="w-5 h-5 me-2.5"
        aria-hidden="true"
        xmlns="http://www.w3.org/2000/svg"
        fill="currentColor"
        viewBox="0 0 20 20"
      >
        <path d="M14.707 7.793a1 1 0 0 0-1.414 0L11 10.086V1.5a1 1 0 0 0-2 0v8.586L6.707 7.793a1 1 0 1 0-1.414 1.414l4 4a1 1 0 0 0 1.416 0l4-4a1 1 0 0 0-.002-1.414Z" />
        <path d="M18 12h-2.55l-2.975 2.975a3.5 3.5 0 0 1-4.95 0L4.55 12H2a2 2 0 0 0-2 2v4a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-4a2 2 0 0 0-2-2Zm-3 5a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z" />
      </svg>
      <span className="sr-only">Download</span>
      {nombre}
    </button>
  );
}

function Sorpresa() {
  const [currentIndex, setCurrentIndex] = useState(0);
  const [visible, setVisible] = useState(true);
  const [phase, setPhase] = useState("cycling");
  const fadeDuration = 500; // ms

  useEffect(() => {
    let timer;
    if (phase === "cycling") {
      if (currentIndex < TEXTOS.length - 1) {
        timer = setTimeout(() => {
          setVisible(false);
          setTimeout(() => {
            setCurrentIndex(currentIndex + 1);
            setVisible(true);
          }, fadeDuration);
        }, 2450);
      } else {
        timer = setTimeout(() => {
          setPhase("downloadFrases");
        }, 2450);
      }
    }
    return () => clearTimeout(timer);
  }, [currentIndex, phase]);

  if (phase === "cycling") {
    return (
      <div className="flex items-center justify-center h-screen w-full">
        <h1
          className={`transition-opacity duration-500 text-center ${
            visible ? "opacity-100" : "opacity-0"
          } text-3xl font-bold`}
        >
          {TEXTOS[currentIndex]}
        </h1>
      </div>
    );
  }

  if (phase === "downloadFrases") {
    return (
      <div className="flex flex-col items-center justify-center h-screen space-y-4">
        <h1 className="text-3xl text-center font-bold">
          {TEXTOS[TEXTOS.length - 1]}
        </h1>
        <BtnDescargar
          link="./frases.png"
          nombre="Descargar :)"
          onDownload={() => setPhase("final")}
        />
      </div>
    );
  }

  if (phase === "final") {
    return (
      <div className="flex flex-col items-center justify-center h-screen space-y-4">
        <h1 className="text-3xl font-bold text-center">{ULTIMOTEXTO}</h1>
        <BtnDescargar link="./carta.png" nombre="Descargar Carta" />
      </div>
    );
  }

  return null;
}

export default function All() {
  const [showSorpresa, setShowSorpresa] = useState(false);

  const handleSorpresa = () => {
    const audio = new Audio("./song.mp3");
    audio.play();
    setShowSorpresa(true);
  };

  return (
    <>
      {showSorpresa ? <Sorpresa /> : <Contador onSorpresa={handleSorpresa} />}
    </>
  );
}